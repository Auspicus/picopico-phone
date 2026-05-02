#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]

use core::mem;

use cyw43::JoinOptions;
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr};
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{self, Common, InterruptHandler, Pio, StateMachine, StateMachineTx};
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};
use embassy_rp::pwm::{Pwm, SetDutyCycle};
use embassy_rp::{bind_interrupts, dma};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use picopico_phone::music::{connection_established, connection_lost, jingle_bells};
use static_cell::StaticCell;
// use picopico_phone::net::{self, Cyw43Peripherals};
use {defmt_rtt as _, panic_probe as _};

enum MusicCommand {
    Ring,
    Connected,
    Disconnected,
}

static MUSIC_CHANNEL: Channel<CriticalSectionRawMutex, MusicCommand, 1> = Channel::new();

// #[embassy_executor::task]
// async fn music_task(mut pwm: Pwm<'static>) {
//     loop {
//         match MUSIC_CHANNEL.receive().await {
//             MusicCommand::Ring => {
//                 if jingle_bells(&mut pwm).await.is_err() {
//                     warn!("failed to play ring");
//                 }
//             }
//             MusicCommand::Connected => {
//                 if connection_established(&mut pwm).await.is_err() {
//                     warn!("failed to play connected");
//                 }
//             }
//             MusicCommand::Disconnected => {
//                 if connection_lost(&mut pwm).await.is_err() {
//                     warn!("failed to play disconnected");
//                 }
//             }
//         }
//     }
// }

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"WiFi Client"),
    embassy_rp::binary_info::rp_program_description!(
        c"Joins a WiFi network (cyw43) and opens a TCP socket with the access point."
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

const WIFI_NETWORK: &str = "cyw43"; // change to your network SSID
const WIFI_PASSWORD: &str = "password"; // change to your network password
const CYW43_GPIO_LED: u8 = 0;
const SAMPLE_RATE_HZ: u32 = 48_000;
const BIT_DEPTH: u32 = 16;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let samples = include_bytes!("../../../audio/output.raw");

    // Setup pio state machine for i2s output
    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, Irqs);

    let bit_clock_pin = p.PIN_18;
    let left_right_clock_pin = p.PIN_19;
    let data_pin = p.PIN_20;

    // drive high the sd for stereo l+r/2
    let mut sd_pin = Output::new(p.PIN_21, Level::High);
    sd_pin.set_high();

    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        p.DMA_CH0,
        Irqs,
        data_pin,
        bit_clock_pin,
        left_right_clock_pin,
        SAMPLE_RATE_HZ,
        BIT_DEPTH,
        &program,
    );
    i2s.start();

    // create two audio buffers (back and front) which will take turns being
    // filled with new audio data and being sent to the pio fifo using dma
    const BUFFER_SIZE: usize = 960;
    static DMA_BUFFER: StaticCell<[u32; BUFFER_SIZE * 2]> = StaticCell::new();
    let dma_buffer = DMA_BUFFER.init_with(|| [0u32; BUFFER_SIZE * 2]);
    let (mut back_buffer, mut front_buffer) = dma_buffer.split_at_mut(BUFFER_SIZE);
    let mut byte_idx = 0;

    loop {
        // trigger transfer of front buffer data to the pio fifo
        // but don't await the returned future, yet
        let dma_future = i2s.write(front_buffer);

        const FRAME_BYTES: usize = 4;

        // fill back buffer with fresh audio samples before awaiting the dma future
        for s in back_buffer.iter_mut() {
            let frame: [u8; 4] = samples[byte_idx..byte_idx + FRAME_BYTES]
                .try_into()
                .unwrap();
            *s = u32::from_le_bytes(frame);

            byte_idx += FRAME_BYTES;
            if byte_idx >= samples.len() {
                byte_idx = 0; // loop the clip
            }
        }

        // now await the dma future. once the dma finishes, the next buffer needs to be queued
        // within DMA_DEPTH / SAMPLE_RATE = 8 / 48000 seconds = 166us
        dma_future.await;
        mem::swap(&mut back_buffer, &mut front_buffer);
    }
}

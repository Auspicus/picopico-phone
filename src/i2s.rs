use core::mem;

use embassy_executor::Spawner;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{DMA_CH1, PIN_18, PIN_19, PIN_20, PIN_21, PIO1},
    pio::Pio,
    pio_programs::i2s::{PioI2sOut, PioI2sOutProgram},
    Peri,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use static_cell::StaticCell;

use crate::Irqs;

pub struct I2sPeripherals {
    pub pio_1: Peri<'static, PIO1>,
    pub dma_ch1: Peri<'static, DMA_CH1>,
    pub pin_18: Peri<'static, PIN_18>,
    pub pin_19: Peri<'static, PIN_19>,
    pub pin_20: Peri<'static, PIN_20>,
    pub pin_21: Peri<'static, PIN_21>,
}

const SAMPLE_RATE_HZ: u32 = 48_000;
const BIT_DEPTH: u32 = 16;

pub enum MusicCommand {
    Ring,
    Connected,
    Disconnected,
}

pub static MUSIC_CHANNEL: Channel<CriticalSectionRawMutex, MusicCommand, 1> = Channel::new();

pub fn init_i2s(spawner: Spawner, p: I2sPeripherals) -> () {
    // Setup pio state machine for i2s output
    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.pio_1, Irqs);

    let bit_clock_pin = p.pin_18;
    let left_right_clock_pin = p.pin_19;
    let data_pin = p.pin_20;

    // drive high the sd for stereo l+r/2
    let mut sd_pin = Output::new(p.pin_21, Level::High);
    sd_pin.set_high();

    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        p.dma_ch1,
        Irqs,
        data_pin,
        bit_clock_pin,
        left_right_clock_pin,
        SAMPLE_RATE_HZ,
        BIT_DEPTH,
        &program,
    );
    i2s.start();

    let Ok(i2s_token) = i2s_task(i2s) else {
        defmt::panic!("failed to create i2s task");
    };
    spawner.spawn(i2s_token);
}

async fn play(i2s: &mut PioI2sOut<'static, PIO1, 0>, mut front_buffer: &mut [u32], mut back_buffer: &mut [u32], bytes: &[u8]) {
    let (chunks, _) = bytes.as_chunks::<CHUNK_SIZE>();
    for chunk in chunks {
        // trigger transfer of front buffer data to the pio fifo
        // but don't await the returned future, yet
        let dma_future = i2s.write(front_buffer);
        let mut idx = 0;

        // fill back buffer with fresh audio samples before awaiting the dma future
        let (frame_byte_chunks, _) = chunk.as_chunks::<FRAME_BYTES>();
        for frame_bytes in frame_byte_chunks {
            let frame: [u8; FRAME_BYTES] = frame_bytes.clone();
            if let Some(s) = back_buffer.get_mut(idx) {
                *s = u32::from_le_bytes(frame);
            };
            idx += 1;
        }

        // now await the dma future. once the dma finishes, the next buffer needs to be queued
        // within DMA_DEPTH / SAMPLE_RATE = 8 / 48000 seconds = 166us
        dma_future.await;
        mem::swap(&mut back_buffer, &mut front_buffer);
    }
}

const BUFFER_SIZE: usize = 960;
const FRAME_BYTES: usize = 4;
const CHUNK_SIZE: usize = BUFFER_SIZE * FRAME_BYTES;

#[embassy_executor::task]
pub async fn i2s_task(mut i2s: PioI2sOut<'static, PIO1, 0>) {
    let ring = include_bytes!("../audio/ding-dong.raw");
    let disconnected = include_bytes!("../audio/disconnected.raw");
    let connected = include_bytes!("../audio/connected.raw");

    // create two audio buffers (back and front) which will take turns being
    // filled with new audio data and being sent to the pio fifo using dma
    static DMA_BUFFER: StaticCell<[u32; BUFFER_SIZE * 2]> = StaticCell::new();
    let dma_buffer = DMA_BUFFER.init_with(|| [0u32; BUFFER_SIZE * 2]);
    let (back_buffer, front_buffer) = dma_buffer.split_at_mut(BUFFER_SIZE);

    loop {
        let cmd = MUSIC_CHANNEL.receive().await;
        match cmd {
            MusicCommand::Ring => {
                play(&mut i2s, front_buffer, back_buffer, ring).await;
            },
            MusicCommand::Connected => {
                play(&mut i2s, front_buffer, back_buffer, connected).await;
            },
            MusicCommand::Disconnected => {
                play(&mut i2s, front_buffer, back_buffer, disconnected).await;
            },
        }
    }
}

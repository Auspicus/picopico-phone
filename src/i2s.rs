use embassy_executor::Spawner;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{DMA_CH1, PIN_18, PIN_19, PIN_20, PIN_21, PIO1},
    pio::Pio,
    pio_programs::i2s::{PioI2sOut, PioI2sOutProgram},
    Peri,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

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

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum MusicCommand {
    Ring,
    Connected,
    Disconnected,
}

pub static MUSIC_CHANNEL: Channel<CriticalSectionRawMutex, MusicCommand, 1> = Channel::new();

pub fn init_i2s(spawner: Spawner, p: I2sPeripherals) -> () {
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

/// Play audio by DMA-ing directly from flash — no RAM buffer needed.
///
/// The audio data lives in .rodata (flash). A single DMA transfer covers the
/// entire clip with no inter-buffer handoffs, so there is no FIFO underrun risk
/// mid-clip regardless of what other tasks are doing.
///
/// The linker aligns .rodata to ≥4 bytes, so the align_to::<u32> cast will
/// always have an empty prefix/suffix for correctly-sized audio files.
async fn play(i2s: &mut PioI2sOut<'static, PIO1, 0>, bytes: &cyw43::Aligned<cyw43::A4, [u8]>) {
    // Aligned<A4, _> guarantees 4-byte alignment, so align_to::<u32> will
    // always produce an empty prefix/suffix for correctly-sized audio files.
    let (prefix, words, suffix) = unsafe { bytes.align_to::<u32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        defmt::error!("audio data wrong length — skipping");
        return;
    }
    i2s.write(words).await;
}

#[embassy_executor::task]
pub async fn i2s_task(mut i2s: PioI2sOut<'static, PIO1, 0>) {
    let ring = cyw43::aligned_bytes!("../audio/ring.raw");
    let disconnected = cyw43::aligned_bytes!("../audio/disconnected.raw");
    let connected = cyw43::aligned_bytes!("../audio/connected.raw");

    let mut last: Option<MusicCommand> = None;

    loop {
        let cmd = MUSIC_CHANNEL.receive().await;
        match cmd {
            MusicCommand::Ring => {
                play(&mut i2s, ring).await;
            },
            MusicCommand::Connected => {
                if last.is_none_or(|c| c != MusicCommand::Connected) {
                    play(&mut i2s, connected).await;
                }
            },
            MusicCommand::Disconnected => {
                if last.is_none_or(|c| c != MusicCommand::Disconnected) {
                    play(&mut i2s, disconnected).await;
                }
            },
        }
        last = Some(cmd);
    }
}

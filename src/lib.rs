#![no_std]
#![allow(async_fn_in_trait)]
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]

use embassy_rp::{
    bind_interrupts, dma,
    peripherals::{DMA_CH0, DMA_CH1, PIO0, PIO1},
    pio,
};

bind_interrupts!(pub struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    PIO1_IRQ_0 => pio::InterruptHandler<PIO1>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
});

pub mod i2s;
pub mod net;

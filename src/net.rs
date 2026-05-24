use cyw43::{aligned_bytes, Control};
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use embassy_executor::Spawner;
use embassy_net::{Ipv4Cidr, Stack, StackResources};
use embassy_rp::{
    clocks::RoscRng,
    dma::{self},
    gpio::{Level, Output},
    peripherals::{DMA_CH0, PIN_23, PIN_24, PIN_25, PIN_29, PIO0},
    pio::Pio,
    Peri,
};
use static_cell::StaticCell;

use crate::Irqs;

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

pub struct Cyw43Peripherals {
    pub pin_23: Peri<'static, PIN_23>,
    pub pin_24: Peri<'static, PIN_24>,
    pub pin_25: Peri<'static, PIN_25>,
    pub pin_29: Peri<'static, PIN_29>,
    pub pio_0: Peri<'static, PIO0>,
    pub dma_ch0: Peri<'static, DMA_CH0>,
}

pub async fn init_cyw43(
    spawner: Spawner,
    p: Cyw43Peripherals,
    ip: Ipv4Cidr,
) -> (Stack<'static>, Control<'static>) {
    let fw = aligned_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = aligned_bytes!("../cyw43-firmware/43439A0_clm.bin");
    let nvram = aligned_bytes!("../cyw43-firmware/nvram_rp2040.bin");

    let pwr = Output::new(p.pin_23, Level::Low);
    let cs = Output::new(p.pin_25, Level::High);
    let mut pio = Pio::new(p.pio_0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.pin_24,
        p.pin_29,
        dma::Channel::new(p.dma_ch0, Irqs),
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw, nvram).await;
    let Ok(runner_spawn_token) = cyw43_task(runner) else {
        defmt::panic!("failed to create cyw43 task")
    };
    spawner.spawn(runner_spawn_token);

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::None)
        .await;

    let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: ip,
        gateway: None,
        dns_servers: Default::default(),
    });

    let mut rng = RoscRng;
    let seed = rng.next_u64();

    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    );

    let Ok(net_spawn_token) = net_task(runner) else {
        defmt::panic!("failed to create net task")
    };
    spawner.spawn(net_spawn_token);

    (stack, control)
}

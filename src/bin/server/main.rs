#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]

use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, StackResources};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_time::{Duration, Instant};
use embedded_io_async::Write;
use picopico_phone::net;
use {defmt_rtt as _, panic_probe as _};

pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"WiFi AP"),
    embassy_rp::binary_info::rp_program_description!(
        c"Starts a simple TCP server on a self-hosted WiFi AP called cyw43."
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let (stack, mut control) = net::init_cyw43(
        spawner,
        p.PIN_23,
        p.PIN_24,
        p.PIN_25,
        p.PIN_29,
        p.PIO0,
        p.DMA_CH0,
        embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::new(169, 254, 1, 1), 16),
    )
    .await;

    let mut trigger = Input::new(p.PIN_0, Pull::Up);
    control.start_ap_wpa2("cyw43", "password", 5).await;

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(20)));
        socket.set_keep_alive(Some(Duration::from_secs(10)));

        control.gpio_set(0, false).await;
        info!("listening on tcp/169.254.1.1:1234");
        if let Err(e) = socket.accept(1234).await {
            warn!("accept error: {:?}", e);
            continue;
        }

        info!("received connection from {:?}", socket.remote_endpoint());
        control.gpio_set(0, true).await;
        let mut last_sent: Instant = Instant::from_ticks(0);

        loop {
            trigger.wait_for_rising_edge().await;
            info!("rising edge detected");
            let now = Instant::now();
            if now.duration_since(last_sent).as_millis() > 5000 {
                last_sent = now;
                match socket.write_all(b"high\n").await {
                    Ok(()) => {}
                    Err(e) => {
                        warn!("write error: {:?}", e);
                        break;
                    }
                }
            }
        }
    }
}

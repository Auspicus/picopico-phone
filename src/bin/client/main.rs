#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]

use cyw43::JoinOptions;
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr, StackResources};
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::pwm::{Pwm, SetDutyCycle};
use embassy_time::{Duration, Timer};
use heapless::Vec;
use picopico_phone::music::ode_to_joy;
use picopico_phone::net;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"WiFi Client"),
    embassy_rp::binary_info::rp_program_description!(
        c"Joins a WiFi network (cyw43) and opens a TCP socket with the access point."
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

const WIFI_NETWORK: &str = "cyw43"; // change to your network SSID
const WIFI_PASSWORD: &str = "password"; // change to your network password

#[embassy_executor::main]
async fn main(mut spawner: Spawner) {
    let mut p = embassy_rp::init(Default::default());
    let (stack, mut control) = net::get_network_stack(
        &mut spawner,
        &mut p,
        Ipv4Cidr::new(Ipv4Address::new(169, 254, 1, 2), 16),
    )
    .await;

    while let Err(err) = control
        .join(WIFI_NETWORK, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
        .await
    {
        info!("join failed with status={}", err.status);
    }

    let mut pwm = Pwm::new_output_a(p.PWM_SLICE2, p.PIN_4, picopico_phone::music::tone(1024));
    if pwm.set_duty_cycle(0).is_err() {
        warn!("failed to set initial duty cycle")
    }

    info!("waiting for link...");
    stack.wait_link_up().await;
    info!("after link up...");

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    info!("created buffers...");
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    socket.set_timeout(Some(Duration::from_secs(20)));
    socket.set_keep_alive(Some(Duration::from_secs(10)));
    while let Err(e) = socket
        .connect(IpEndpoint::new(IpAddress::v4(169, 254, 1, 1), 1234))
        .await
    {
        info!("failed to connect due to {:?}", e);
        Timer::after(Duration::from_millis(1000)).await;
    }

    info!("waiting for message...");
    let mut msg_buffer = [0; 4096];
    loop {
        match socket.read(&mut msg_buffer).await {
            Ok(bytes_read) => {
                if bytes_read == 0 {
                    break;
                }

                if let Err(e) = ode_to_joy(&mut pwm).await {
                    warn!("failed to play song due to error {:?}", e);
                }
            }
            Err(e) => {
                warn!("failed to read from socket due to error {:?}", e);
                break;
            }
        }
    }
}

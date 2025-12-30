#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]

use cyw43::JoinOptions;
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr};
use embassy_rp::pwm::{Pwm, SetDutyCycle};
use embassy_time::{Duration, Instant, Timer};
use picopico_phone::music::{jingle_bells, ode_to_joy};
use picopico_phone::net::{self, Cyw43Peripherals};
use {defmt_rtt as _, panic_probe as _};

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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let (stack, mut control) = net::init_cyw43(
        spawner,
        Cyw43Peripherals {
            pin_23: p.PIN_23,
            pin_24: p.PIN_24,
            pin_25: p.PIN_25,
            pin_29: p.PIN_29,
            pio_0: p.PIO0,
            dma_ch0: p.DMA_CH0,
        },
        Ipv4Cidr::new(Ipv4Address::new(169, 254, 1, 2), 16),
    )
    .await;

    let mut pwm = Pwm::new_output_a(p.PWM_SLICE2, p.PIN_4, picopico_phone::music::tone(1024));
    if pwm.set_duty_cycle(0).is_err() {
        warn!("failed to set initial duty cycle")
    }

    if jingle_bells(&mut pwm).await.is_err() {
        warn!("failed to play song due to error");
    }

    let mut last_ring: Instant = Instant::from_ticks(0);
    'network: loop {
        control.leave().await;
        while let Err(err) = control
            .join(WIFI_NETWORK, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            warn!("join failed with status={}", err.status);
        }
        stack.wait_link_up().await;
        if jingle_bells(&mut pwm).await.is_err() {
            warn!("failed to play song due to error");
        }

        let mut rx_buffer = [0; 1024];
        let mut tx_buffer = [0; 1024];
        let mut msg_buffer = [0; 1024];
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(20)));
        socket.set_keep_alive(Some(Duration::from_secs(10)));

        loop {
            control.gpio_set(CYW43_GPIO_LED, false).await;
            while let Err(e) = socket
                .connect(IpEndpoint::new(IpAddress::v4(169, 254, 1, 1), 1234))
                .await
            {
                warn!("failed to connect due to {:?}", e);
                Timer::after(Duration::from_millis(1000)).await;
            }
            control.gpio_set(CYW43_GPIO_LED, true).await;

            loop {
                match socket.read(&mut msg_buffer).await {
                    Ok(bytes_read) => {
                        if bytes_read == 0 {
                            break 'network;
                        }

                        let now = Instant::now();
                        if now.duration_since(last_ring).as_millis() > 5000 {
                            last_ring = now;
                            if jingle_bells(&mut pwm).await.is_err() {
                                warn!("failed to play song due to error");
                            }
                        }
                    }
                    Err(e) => {
                        warn!("failed to read from socket due to error {:?}", e);
                        break 'network;
                    }
                }
            }
        }
    }
}

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
use embassy_time::{Duration, Timer};
use picopico_phone::i2s::{self, I2sPeripherals, init_i2s};
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
    init_i2s(
        spawner,
        I2sPeripherals {
            pio_1: p.PIO1,
            dma_ch1: p.DMA_CH1,
            pin_18: p.PIN_18,
            pin_19: p.PIN_19,
            pin_20: p.PIN_20,
            pin_21: p.PIN_21,
        },
    );

    loop {
        debug!("connecting");
        control.gpio_set(CYW43_GPIO_LED, false).await;
        while let Err(err) = control
            .join(WIFI_NETWORK, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            warn!("join failed with status={}", err);
            Timer::after(Duration::from_millis(1000)).await;
        }
        stack.wait_link_up().await;

        let mut rx_buffer = [0; 1024];
        let mut tx_buffer = [0; 1024];
        let mut msg_buffer = [0; 1024];
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(20)));
        socket.set_keep_alive(Some(Duration::from_secs(10)));

        'socket: loop {
            debug!("establishing socket");
            if let Err(e) = socket
                .connect(IpEndpoint::new(IpAddress::v4(169, 254, 1, 1), 1234))
                .await
            {
                warn!("failed to connect due to {:?}", e);
                Timer::after(Duration::from_millis(1000)).await;
                break 'socket;
            }
            i2s::MUSIC_CHANNEL.send(i2s::MusicCommand::Connected).await;
            control.gpio_set(CYW43_GPIO_LED, true).await;

            loop {
                match socket.read(&mut msg_buffer).await {
                    Ok(bytes_read) => {
                        if bytes_read == 0 {
                            break 'socket;
                        }

                        let _ = i2s::MUSIC_CHANNEL.try_send(i2s::MusicCommand::Ring);
                    }
                    Err(e) => {
                        warn!("failed to read from socket due to error {:?}", e);
                        break 'socket;
                    }
                }
            }
        }

        debug!("connection lost");
        control.gpio_set(CYW43_GPIO_LED, false).await;
        i2s::MUSIC_CHANNEL.send(i2s::MusicCommand::Disconnected).await;
        control.leave().await;

        // backoff and retry.
        Timer::after(Duration::from_millis(5_000)).await;
    }
}

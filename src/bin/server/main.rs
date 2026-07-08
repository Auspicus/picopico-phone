#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::tcp::TcpSocket;
use embassy_rp::gpio::{Input, Level, Pull};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use embedded_io_async::Write;
use picopico_phone::net::{self, Cyw43Peripherals};
use {defmt_rtt as _, panic_probe as _};

pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"WiFi AP"),
    embassy_rp::binary_info::rp_program_description!(
        c"Starts a simple TCP server on a self-hosted WiFi AP called cyw43."
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

const CYW43_GPIO_LED: u8 = 0;

/// Signals a confirmed trigger event to the socket task.
static TRIGGER_CHANNEL: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();

/// Watches PIN_5 for high→low transitions (with debounce) and sends
/// a `()` to TRIGGER_CHANNEL for each confirmed press. Runs forever,
/// independently of socket state so no button events are ever missed.
#[embassy_executor::task]
async fn trigger_task(mut trigger: Input<'static>) {
    loop {
        trigger.wait_for_high().await;

        Timer::after(Duration::from_millis(500)).await;
        if trigger.get_level() != Level::High {
            debug!("high for less than 500ms!");
            continue;
        }

        trigger.wait_for_low().await;

        Timer::after(Duration::from_millis(500)).await;
        if trigger.get_level() != Level::Low {
            debug!("low for less than 500ms!");
            continue;
        }

        TRIGGER_CHANNEL.send(()).await;
    }
}

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
        embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::new(169, 254, 1, 1), 16),
    )
    .await;

    let trigger = Input::new(p.PIN_5, Pull::Up);
    control.start_ap_wpa2("cyw43", "password", 5).await;
    control.gpio_set(CYW43_GPIO_LED, true).await;

    let Ok(trigger_token) = trigger_task(trigger) else {
        defmt::panic!("failed to create trigger task");
    };
    spawner.spawn(trigger_token);

    loop {
        let mut rx_buffer = [0; 1024];
        let mut tx_buffer = [0; 1024];
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(30)));
        socket.set_keep_alive(Some(Duration::from_secs(3)));

        info!("listening on tcp/169.254.1.1:1234");
        if let Err(e) = socket.accept(1234).await {
            warn!("accept error: {:?}", e);
            continue;
        }

        info!("client connected");

        let mut dead = [0u8; 1];
        loop {
            // Race a trigger event against the socket closing. The server never
            // expects to receive data, so a read returning 0 or an error means
            // the client has disconnected — break immediately without waiting
            // for the next button press.
            match select(TRIGGER_CHANNEL.receive(), socket.read(&mut dead)).await {
                Either::First(()) => {
                    if socket.write_all(b"e").await.is_err() {
                        warn!("write error — dropping connection");
                        break;
                    }
                }
                Either::Second(_) => {
                    info!("client disconnected");
                    break;
                }
            }
        }

        info!("connection lost, waiting for new client");
    }
}

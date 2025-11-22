#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]

//! This example uses the RP Pico W board Wifi chip (cyw43).
//! Creates an Access point Wifi network and creates a TCP endpoint on port 1234.

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
use embassy_rp::pwm::{Config, Pwm, SetDutyCycle};
use embassy_time::{Duration, Timer};
use embedded_io_async::Write;
use heapless::Vec;
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

const WIFI_NETWORK: &str = "cyw43"; // change to your network SSID
const WIFI_PASSWORD: &str = "password"; // change to your network password
const SYS_CLOCK: u64 = 150_000_000;
const DIV_INT: u8 = 64;

fn into_cfg(target_frequency: u64, duty_cycle: f64) -> Config {
    let mut o = Config::default();
    o.enable = true;
    o.top = (SYS_CLOCK / (target_frequency * DIV_INT as u64)) as u16;
    o.compare_a = (o.top as f64 * duty_cycle) as u16;
    o.divider = DIV_INT.into();
    return o;
}

// #[embassy_executor::task]
async fn ring_buzzer(pwm: &mut Pwm<'_>, tone_a_cfg: Config, tone_b_cfg: Config) {
    pwm.set_duty_cycle_percent(50).unwrap();
    Timer::after(Duration::from_millis(500)).await;
    pwm.set_config(&tone_a_cfg);
    pwm.set_duty_cycle(0).unwrap();
    Timer::after(Duration::from_millis(100)).await;
    pwm.set_duty_cycle_percent(50).unwrap();
    Timer::after(Duration::from_millis(750)).await;
    pwm.set_config(&tone_b_cfg);
    pwm.set_duty_cycle(0).unwrap();
    Timer::after(Duration::from_millis(1_000)).await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let fw = include_bytes!("../../../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../../../cyw43-firmware/43439A0_clm.bin");

    info!("client starting...");
    let tone_a_cfg = into_cfg(4698, 0.5);
    let tone_b_cfg = into_cfg(4186, 0.5);
    let p = embassy_rp::init(Default::default());
    let mut rng = RoscRng;
    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pwm = Pwm::new_output_a(p.PWM_SLICE0, p.PIN_0, tone_a_cfg.clone());
    pwm.set_duty_cycle(0)
        .expect("Failed to set initial duty cycle to 0.");

    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    spawner
        .spawn(cyw43_task(runner))
        .expect("Failed to spawn cyw43 task");

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    // Use static IP configuration instead of DHCP
    let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(169, 254, 1, 2), 16),
        dns_servers: Vec::new(),
        gateway: None,
    });

    // Generate random seed
    let seed = rng.next_u64();

    // Init network stack
    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner
        .spawn(net_task(runner))
        .expect("Failed to spawn net task");

    while let Err(err) = control
        .join(WIFI_NETWORK, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
        .await
    {
        info!("join failed with status={}", err.status);
    }

    info!("waiting for link...");
    stack.wait_link_up().await;

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    socket
        .connect(IpEndpoint::new(IpAddress::v4(169, 254, 1, 1), 1234))
        .await
        .expect("Failed to connect");
    socket.set_timeout(Some(Duration::from_secs(20)));
    socket.set_keep_alive(Some(Duration::from_secs(10)));

    let mut msg_buffer = [0; 4096];
    loop {
        match socket.read(&mut msg_buffer).await {
            Ok(_) => ring_buzzer(&mut pwm, tone_a_cfg.clone(), tone_b_cfg.clone()).await,
            Err(e) => {
                warn!("read error: {:?}", e);
                break;
            }
        }
    }
}

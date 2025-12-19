use embassy_rp::pwm::{Config, Pwm, PwmError, SetDutyCycle};
use embassy_time::{Duration, Timer};

const SYS_CLOCK: u64 = 150_000_000;
const DIV_INT: u8 = 64;

pub struct Note<'a> {
    tone: &'a Config,
    time: u64,
}

pub fn tone(target_frequency: u64) -> Config {
    const DUTY_CYCLE: f64 = 0.5;
    let mut o = Config::default();
    o.enable = true;
    o.top = (SYS_CLOCK / (target_frequency * DIV_INT as u64)) as u16;
    o.compare_a = (o.top as f64 * DUTY_CYCLE) as u16;
    o.divider = DIV_INT.into();
    o
}

pub async fn ode_to_joy(pwm: &mut Pwm<'_>) -> Result<(), PwmError> {
    let c = tone(1047);
    let d = tone(1175);
    let e = tone(1319);
    let f = tone(1397);
    let g = tone(1568);
    let song: [&Note; 30] = [
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &f,
            time: 250,
        },
        &Note {
            tone: &g,
            time: 250,
        }, // bar 1
        &Note {
            tone: &g,
            time: 250,
        },
        &Note {
            tone: &f,
            time: 250,
        },
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &d,
            time: 250,
        }, // bar 2
        &Note {
            tone: &c,
            time: 250,
        },
        &Note {
            tone: &c,
            time: 250,
        },
        &Note {
            tone: &d,
            time: 250,
        },
        &Note {
            tone: &e,
            time: 250,
        }, // bar 3
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &d,
            time: 250,
        },
        &Note {
            tone: &d,
            time: 500,
        }, // bar 4
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &f,
            time: 250,
        },
        &Note {
            tone: &g,
            time: 250,
        }, // bar 5
        &Note {
            tone: &g,
            time: 250,
        },
        &Note {
            tone: &f,
            time: 250,
        },
        &Note {
            tone: &e,
            time: 250,
        },
        &Note {
            tone: &d,
            time: 250,
        }, // bar 6
        &Note {
            tone: &c,
            time: 250,
        },
        &Note {
            tone: &c,
            time: 250,
        },
        &Note {
            tone: &d,
            time: 250,
        },
        &Note {
            tone: &e,
            time: 250,
        }, // bar 7
        &Note {
            tone: &d,
            time: 250,
        },
        &Note {
            tone: &c,
            time: 250,
        },
        &Note {
            tone: &c,
            time: 500,
        }, // bar 8
    ];
    for note in song {
        pwm.set_duty_cycle_percent(50)?;
        pwm.set_config(note.tone);
        Timer::after(Duration::from_millis(note.time)).await;
        pwm.set_duty_cycle_percent(0)?;
        Timer::after(Duration::from_millis(50)).await;
    }

    Ok(())
}

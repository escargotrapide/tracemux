use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};

use crate::scenario::Scenario;
use crate::transcript::{Direction, Transcript};

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) port: String,
    pub(crate) baud: u32,
    pub(crate) data_bits: u8,
    pub(crate) parity: String,
    pub(crate) stop_bits: u8,
    pub(crate) flow: String,
    pub(crate) read_timeout_ms: u64,
}

pub(crate) async fn run(
    config: Config,
    scenario: Scenario,
    transcript: Arc<Transcript>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || run_blocking(&config, &scenario, &transcript))
        .await
        .context("joining serial worker task")?
}

fn run_blocking(config: &Config, scenario: &Scenario, transcript: &Arc<Transcript>) -> Result<()> {
    let mut port = serialport::new(&config.port, config.baud)
        .data_bits(data_bits(config.data_bits)?)
        .parity(parity(&config.parity)?)
        .stop_bits(stop_bits(config.stop_bits)?)
        .flow_control(flow_control(&config.flow)?)
        .timeout(Duration::from_millis(config.read_timeout_ms))
        .open()
        .with_context(|| format!("opening serial port {}", config.port))?;
    println!("wanlogger-virt-peer serial opened {}", config.port);
    transcript.record_event("serial", Some(&config.port), "opened")?;

    let writer =
        Arc::new(Mutex::new(port.try_clone().with_context(|| {
            format!("cloning serial port {}", config.port)
        })?));
    let script_writer = writer.clone();
    let script_scenario = scenario.clone();
    let script_transcript = transcript.clone();
    let script_port = config.port.clone();
    let script_thread = std::thread::spawn(move || {
        script_loop_blocking(
            &script_writer,
            &script_scenario,
            &script_transcript,
            &script_port,
        )
    });

    let read_result = read_loop_blocking(&mut *port, &writer, scenario, transcript, &config.port);
    let script_result = script_thread
        .join()
        .map_err(|_| anyhow::anyhow!("serial script thread panicked"))?;
    script_result?;
    read_result
}

fn script_loop_blocking(
    writer: &Arc<Mutex<Box<dyn SerialPort>>>,
    scenario: &Scenario,
    transcript: &Transcript,
    port: &str,
) -> Result<()> {
    let payloads = scenario.scripted_payloads();
    if !payloads.is_empty() && !scenario.initial_delay().is_zero() {
        std::thread::sleep(scenario.initial_delay());
    }
    let last = payloads.len().saturating_sub(1);
    for (idx, payload) in payloads.iter().enumerate() {
        write_payload_blocking(writer, scenario, transcript, port, payload)?;
        if idx != last && !scenario.interval().is_zero() {
            std::thread::sleep(scenario.interval());
        }
    }
    Ok(())
}

fn read_loop_blocking(
    port: &mut dyn SerialPort,
    writer: &Arc<Mutex<Box<dyn SerialPort>>>,
    scenario: &Scenario,
    transcript: &Transcript,
    port_name: &str,
) -> Result<()> {
    let mut buf = vec![0; scenario.read_chunk()];
    let mut last_activity = Instant::now();
    loop {
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                last_activity = Instant::now();
                let inbound = &buf[..n];
                transcript.record_bytes("serial", Direction::In, Some(port_name), inbound)?;
                if let Some(reply) = scenario.echo_payload(inbound) {
                    write_payload_blocking(writer, scenario, transcript, port_name, &reply)?;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                if scenario
                    .idle_timeout()
                    .is_some_and(|timeout| last_activity.elapsed() >= timeout)
                {
                    transcript.record_event("serial", Some(port_name), "idle-timeout")?;
                    break;
                }
            }
            Err(err) => {
                return Err(err).with_context(|| format!("reading serial port {port_name}"))
            }
        }
    }
    Ok(())
}

fn write_payload_blocking(
    writer: &Arc<Mutex<Box<dyn SerialPort>>>,
    scenario: &Scenario,
    transcript: &Transcript,
    port: &str,
    payload: &[u8],
) -> Result<()> {
    for chunk in scenario.chunks(payload) {
        let mut guard = writer
            .lock()
            .map_err(|_| anyhow::anyhow!("serial writer lock poisoned"))?;
        guard
            .write_all(chunk)
            .with_context(|| format!("writing serial port {port}"))?;
        guard
            .flush()
            .with_context(|| format!("flushing serial port {port}"))?;
        drop(guard);
        transcript.record_bytes("serial", Direction::Out, Some(port), chunk)?;
    }
    Ok(())
}

fn data_bits(value: u8) -> Result<DataBits> {
    match value {
        5 => Ok(DataBits::Five),
        6 => Ok(DataBits::Six),
        7 => Ok(DataBits::Seven),
        8 => Ok(DataBits::Eight),
        _ => bail!("data_bits must be 5..=8"),
    }
}

fn parity(value: &str) -> Result<Parity> {
    match value {
        "none" => Ok(Parity::None),
        "even" => Ok(Parity::Even),
        "odd" => Ok(Parity::Odd),
        _ => bail!("parity must be none|even|odd"),
    }
}

fn stop_bits(value: u8) -> Result<StopBits> {
    match value {
        1 => Ok(StopBits::One),
        2 => Ok(StopBits::Two),
        _ => bail!("stop_bits must be 1 or 2"),
    }
}

fn flow_control(value: &str) -> Result<FlowControl> {
    match value {
        "none" => Ok(FlowControl::None),
        "hardware" => Ok(FlowControl::Hardware),
        "software" => Ok(FlowControl::Software),
        _ => bail!("flow must be none|hardware|software"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_option_parsers_accept_expected_values() {
        assert!(matches!(data_bits(8).unwrap(), DataBits::Eight));
        assert!(matches!(parity("none").unwrap(), Parity::None));
        assert!(matches!(parity("even").unwrap(), Parity::Even));
        assert!(matches!(stop_bits(2).unwrap(), StopBits::Two));
        assert!(matches!(
            flow_control("hardware").unwrap(),
            FlowControl::Hardware
        ));
    }

    #[test]
    fn serial_option_parsers_reject_bad_values() {
        assert!(data_bits(9).is_err());
        assert!(parity("bad").is_err());
        assert!(stop_bits(3).is_err());
        assert!(flow_control("bad").is_err());
    }
}

/// QPIWS — Query Device Warning Status
///
/// Response: 32-character binary string, each char is '0' or '1'.
/// Position a0 (leftmost) to a31 (rightmost).
///
/// Severity levels published per warning:
///   0 = OK (bit = 0)
///   1 = Warning (bit = 1, warning type)
///   2 = Fault   (bit = 1, fault type)
///
/// Compound warnings (a9, a10, a11, a16):
///   If a1=1 (inverter fault active) → severity = 2 (Fault)
///   If a1=0                         → severity = 1 (Warning)
///
/// A consolidated `inverter_status` field is also computed (see `status`):
///   0 = all OK
///   1 = at least one Warning, no Faults
///   2 = at least one Fault (highest priority)
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QpiwsData {
    pub warn_pv_loss: u8,                    // a0  Warning
    pub warn_inverter_fault: u8,             // a1  Fault  (also modifies a9,a10,a11,a16)
    pub warn_bus_over: u8,                   // a2  Fault
    pub warn_bus_under: u8,                  // a3  Fault
    pub warn_bus_soft_fail: u8,              // a4  Fault
    pub warn_line_fail: u8,                  // a5  Warning
    pub warn_opv_short: u8,                  // a6  Warning
    pub warn_inverter_voltage_low: u8,       // a7  Fault
    pub warn_inverter_voltage_high: u8,      // a8  Fault
    pub warn_over_temperature: u8,           // a9  Compound
    pub warn_fan_locked: u8,                 // a10 Compound
    pub warn_battery_voltage_high: u8,       // a11 Compound
    pub warn_battery_low_alarm: u8,          // a12 Warning
    pub warn_reserved_13: u8,               // a13 Reserved (always 0)
    pub warn_battery_under_shutdown: u8,     // a14 Warning
    pub warn_battery_derating: u8,          // a15 Warning
    pub warn_overload: u8,                   // a16 Compound
    pub warn_eeprom_fault: u8,              // a17 Fault
    pub warn_inverter_over_current: u8,      // a18 Fault
    pub warn_inverter_soft_fail: u8,         // a19 Fault
    pub warn_self_test_fail: u8,            // a20 Fault
    pub warn_op_dc_voltage_over: u8,         // a21 Fault
    pub warn_bat_open: u8,                  // a22 Fault
    pub warn_current_sensor_fail: u8,        // a23 Fault
    pub warn_battery_short: u8,             // a24 Fault
    pub warn_power_limit: u8,               // a25 Warning
    pub warn_pv_voltage_high: u8,           // a26 Warning
    pub warn_mppt_overload_fault: u8,       // a27 Warning
    pub warn_mppt_overload_warning: u8,     // a28 Warning
    pub warn_battery_too_low_charge: u8,    // a29 Warning
    pub warn_reserved_30: u8,              // a30 Reserved
    pub warn_reserved_31: u8,              // a31 Reserved

    /// Consolidated status: 0=OK, 1=Warning, 2=Fault
    pub inverter_status: u8,
}

pub fn parse(payload: &str) -> Result<QpiwsData> {
    let s = payload.trim();
    if s.len() < 32 {
        bail!(
            "QPIWS: expected 32-char binary string, got {} chars: '{}'",
            s.len(),
            s
        );
    }

    let bits: Vec<bool> = s.chars().map(|c| c == '1').collect();

    let a1 = bits[1]; // inverter_fault — used for compound severity

    let w = |idx: usize, kind: Kind| severity(bits[idx], kind, a1);

    let data = QpiwsData {
        warn_pv_loss: w(0, Kind::Warning),
        warn_inverter_fault: w(1, Kind::Fault),
        warn_bus_over: w(2, Kind::Fault),
        warn_bus_under: w(3, Kind::Fault),
        warn_bus_soft_fail: w(4, Kind::Fault),
        warn_line_fail: w(5, Kind::Warning),
        warn_opv_short: w(6, Kind::Warning),
        warn_inverter_voltage_low: w(7, Kind::Fault),
        warn_inverter_voltage_high: w(8, Kind::Fault),
        warn_over_temperature: w(9, Kind::Compound),
        warn_fan_locked: w(10, Kind::Compound),
        warn_battery_voltage_high: w(11, Kind::Compound),
        warn_battery_low_alarm: w(12, Kind::Warning),
        warn_reserved_13: 0,
        warn_battery_under_shutdown: w(14, Kind::Warning),
        warn_battery_derating: w(15, Kind::Warning),
        warn_overload: w(16, Kind::Compound),
        warn_eeprom_fault: w(17, Kind::Fault),
        warn_inverter_over_current: w(18, Kind::Fault),
        warn_inverter_soft_fail: w(19, Kind::Fault),
        warn_self_test_fail: w(20, Kind::Fault),
        warn_op_dc_voltage_over: w(21, Kind::Fault),
        warn_bat_open: w(22, Kind::Fault),
        warn_current_sensor_fail: w(23, Kind::Fault),
        warn_battery_short: w(24, Kind::Fault),
        warn_power_limit: w(25, Kind::Warning),
        warn_pv_voltage_high: w(26, Kind::Warning),
        warn_mppt_overload_fault: w(27, Kind::Warning),
        warn_mppt_overload_warning: w(28, Kind::Warning),
        warn_battery_too_low_charge: w(29, Kind::Warning),
        warn_reserved_30: 0,
        warn_reserved_31: 0,
        inverter_status: 0, // filled below
    };

    let status = data.status(false);
    Ok(QpiwsData {
        inverter_status: status,
        ..data
    })
}

impl QpiwsData {
    /// Consolidated status: highest severity across all warnings.
    /// `ignore_line_fail`: off-grid installation, where a missing grid
    /// (a5 Line fail, the AC input warning) is the normal state.
    pub fn status(&self, ignore_line_fail: bool) -> u8 {
        let line_fail = if ignore_line_fail { 0 } else { self.warn_line_fail };
        [
            self.warn_pv_loss, self.warn_inverter_fault, self.warn_bus_over,
            self.warn_bus_under, self.warn_bus_soft_fail, line_fail,
            self.warn_opv_short, self.warn_inverter_voltage_low, self.warn_inverter_voltage_high,
            self.warn_over_temperature, self.warn_fan_locked, self.warn_battery_voltage_high,
            self.warn_battery_low_alarm, self.warn_battery_under_shutdown, self.warn_battery_derating,
            self.warn_overload, self.warn_eeprom_fault, self.warn_inverter_over_current,
            self.warn_inverter_soft_fail, self.warn_self_test_fail, self.warn_op_dc_voltage_over,
            self.warn_bat_open, self.warn_current_sensor_fail, self.warn_battery_short,
            self.warn_power_limit, self.warn_pv_voltage_high, self.warn_mppt_overload_fault,
            self.warn_mppt_overload_warning, self.warn_battery_too_low_charge,
        ]
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
    }
}

enum Kind {
    Warning,
    Fault,
    Compound,
}

fn severity(bit: bool, kind: Kind, a1: bool) -> u8 {
    if !bit {
        return 0;
    }
    match kind {
        Kind::Warning => 1,
        Kind::Fault => 2,
        Kind::Compound => {
            if a1 { 2 } else { 1 }
        }
    }
}

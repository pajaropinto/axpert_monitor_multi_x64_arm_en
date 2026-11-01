pub mod watcher;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Write a file atomically: write a temp file next to it, then rename.
/// A crash or power cut mid-write leaves either the old or the new file,
/// never a truncated one (important for configs on the persistent volume).
pub fn write_atomic(path: &str, content: &str) -> Result<()> {
    let tmp = format!("{}.tmp", path);
    std::fs::write(&tmp, content).with_context(|| format!("Cannot write {}", tmp))?;
    std::fs::rename(&tmp, path).with_context(|| format!("Cannot replace {}", path))
}

// Unknown keys are ignored and new keys need a default, so a config file on the
// persistent volume keeps working with both older and newer images.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub system: SystemConfig,
    pub battery: BatteryConfig,
    pub mqtt: MqttConfig,
    #[serde(default)]
    pub web: WebConfig,
    pub inverters: Vec<InverterConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemConfig {
    pub num_inverters: u8,
    #[serde(default = "default_delay_inv")]
    pub delay_between_inverters_ms: u64,
    #[serde(default = "default_delay_cycle")]
    pub delay_between_cycles_ms: u64,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// DC→AC efficiency used to estimate grid input power (%, 100 = ignore losses)
    #[serde(default = "default_inverter_efficiency")]
    pub inverter_efficiency_percent: f64,
    /// Off-grid installation: the AC input warning (Line fail) does not
    /// count for the warning/fault status of any inverter.
    #[serde(default)]
    pub off_grid_installation: bool,
}

fn default_delay_inv() -> u64 { 1000 }
fn default_delay_cycle() -> u64 { 5000 }
fn default_log_level() -> String { "info".to_string() }
fn default_inverter_efficiency() -> f64 { 100.0 }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatteryConfig {
    pub capacity_ah: f64,
    #[serde(default = "default_safety_margin")]
    pub safety_margin_percent: f64,
    #[serde(default = "default_co2_factor")]
    pub co2_factor_kg_per_kwh: f64,
}

fn default_safety_margin() -> f64 { 20.0 }
fn default_co2_factor() -> f64 { 0.233 }

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MqttConfig {
    pub broker_ip: String,
    #[serde(default = "default_mqtt_port")]
    pub broker_port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_topic_prefix")]
    pub topic_prefix: String,
    #[serde(default = "default_ha_prefix")]
    pub ha_discovery_prefix: String,
}

fn default_mqtt_port() -> u16 { 1883 }
fn default_topic_prefix() -> String { "axpert".to_string() }
fn default_ha_prefix() -> String { "homeassistant".to_string() }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebConfig {
    #[serde(default = "default_web_port")]
    pub port: u16,
    #[serde(default = "default_session_timeout")]
    pub session_timeout_seconds: u64,
}

fn default_web_port() -> u16 { 60606 }
fn default_session_timeout() -> u64 { 3600 }

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            port: default_web_port(),
            session_timeout_seconds: default_session_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InverterConfig {
    pub id: u8,
    pub name: String,
    pub tcp_ip: String,
    pub tcp_port: u16,
    pub ha_device_name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read config: {}", path))?;
        let config: AppConfig = toml::from_str(&content)
            .with_context(|| format!("Cannot parse config: {}", path))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks done on load and before saving from the web UI, so an invalid
    /// file is never written (it would stop the app from starting).
    pub fn validate(&self) -> Result<()> {
        if self.system.num_inverters < 1 || self.system.num_inverters > 9 {
            anyhow::bail!("num_inverters must be 1-9, got {}", self.system.num_inverters);
        }
        let mut ids = std::collections::HashSet::new();
        for inv in &self.inverters {
            if !(1..=9).contains(&inv.id) {
                anyhow::bail!("inverter id must be 1-9, got {}", inv.id);
            }
            if !ids.insert(inv.id) {
                anyhow::bail!("inverter id {} is defined more than once", inv.id);
            }
            if inv.tcp_port == 0 {
                anyhow::bail!("inverter {}: tcp_port must not be 0", inv.id);
            }
        }
        let enabled_count = self.inverters.iter().filter(|i| i.enabled).count();
        if enabled_count < self.system.num_inverters as usize {
            anyhow::bail!(
                "Only {} enabled inverter definitions but num_inverters={}",
                enabled_count,
                self.system.num_inverters
            );
        }
        if self.system.delay_between_inverters_ms < 100 {
            anyhow::bail!("delay_between_inverters_ms must be >= 100");
        }
        if self.system.delay_between_cycles_ms < 1000 {
            anyhow::bail!("delay_between_cycles_ms must be >= 1000");
        }
        if !(50.0..=100.0).contains(&self.system.inverter_efficiency_percent) {
            anyhow::bail!("inverter_efficiency_percent must be 50-100");
        }
        if self.battery.capacity_ah <= 0.0 {
            anyhow::bail!("battery.capacity_ah must be > 0");
        }
        if self.battery.safety_margin_percent < 0.0 || self.battery.safety_margin_percent >= 100.0 {
            anyhow::bail!("battery.safety_margin_percent must be 0-99");
        }
        Ok(())
    }

    /// Returns only enabled inverters, sorted by id, up to num_inverters
    pub fn active_inverters(&self) -> Vec<&InverterConfig> {
        let mut inv: Vec<&InverterConfig> = self
            .inverters
            .iter()
            .filter(|i| i.enabled)
            .collect();
        inv.sort_by_key(|i| i.id);
        inv.truncate(self.system.num_inverters as usize);
        inv
    }
}

const INVERTER_PARAM_TEMPLATE: &str = "config.defaults/inv_config.conf";

/// Create config/invNN_config.conf from the template if it does not exist yet
/// (inverter added while running). Existing files are never touched.
pub fn ensure_inverter_param_file(inv_id: u8) -> Result<()> {
    let path = format!("config/inv{:02}_config.conf", inv_id);
    if std::path::Path::new(&path).exists() {
        return Ok(());
    }
    std::fs::copy(INVERTER_PARAM_TEMPLATE, &path)
        .with_context(|| format!("Cannot create {} from {}", path, INVERTER_PARAM_TEMPLATE))?;
    tracing::info!("Created {} from the default template", path);
    Ok(())
}

/// Per-inverter configuration parameters (loaded from invXX_config.conf)
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct InverterParamConfig {
    pub params: InverterParams,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct InverterParams {
    #[serde(default)]
    pub source_priority: Option<String>,
    #[serde(default)]
    pub ac_input_range: Option<String>,
    #[serde(default)]
    pub output_frequency: Option<String>,
    #[serde(default)]
    pub max_charger_current: Option<String>,
    #[serde(default)]
    pub charger_priority: Option<String>,
    #[serde(default)]
    pub max_discharge_current: Option<String>,
    #[serde(default)]
    pub overload_restart: Option<String>,
    #[serde(default)]
    pub overtemp_restart: Option<String>,
    #[serde(default)]
    pub buzzer: Option<String>,
    #[serde(default)]
    pub lcd_escape: Option<String>,
    #[serde(default)]
    pub backlight: Option<String>,
    #[serde(default)]
    pub grid_loss_alarm: Option<String>,
    #[serde(default)]
    pub overload_bypass: Option<String>,
    #[serde(default)]
    pub fault_logging: Option<String>,
    #[serde(default)]
    pub equalization: Option<String>,
}

impl InverterParamConfig {
    pub fn load(inv_id: u8) -> Result<Self> {
        let path = format!("config/inv{:02}_config.conf", inv_id);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Cannot read inverter config: {}", path))?;
        toml::from_str(&content)
            .with_context(|| format!("Cannot parse inverter config: {}", path))
    }

    pub fn save(&self, inv_id: u8) -> Result<()> {
        let path = format!("config/inv{:02}_config.conf", inv_id);
        let content = toml::to_string_pretty(self)
            .context("Cannot serialize inverter config")?;
        write_atomic(&path, &content)
    }
}

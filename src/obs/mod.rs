use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use derivative::Derivative;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use global_hotkey::hotkey::HotKey;
use log::{debug, info, warn};
use once_cell::sync::Lazy;
use secrecy::ExposeSecret;
use secrecy::Secret;
use serde::{Deserialize, Deserializer};
use serde::de::{Error, IntoDeserializer};
use toml::Table;
use winit::event_loop::{ControlFlow, EventLoopBuilder};

#[derive(Derivative, Deserialize)]
#[derivative(Debug)]
pub(crate) struct ClientOptions {
    #[serde(rename = "server")]
    ws_opt: WSServer,
    #[serde(rename = "plugin")]
    plugin_opts: HashMap<String, Table>,
}

pub(crate) struct Client {
    ws: obws::Client,
    plugins: Vec<Box<dyn Plugin>>,
    hotkey_plugins: HashMap<u32, Box<dyn Plugin>>,

    hotkey_mgr: GlobalHotKeyManager,
}

impl Client {
    pub async fn new(config: ClientOptions) -> Result<Self> {
        let ClientOptions { ws_opt, plugin_opts } = config;

        debug!("Connecting to {}:{} with {:?}", ws_opt.host, ws_opt.port, ws_opt.password);
        let ws = obws::Client::connect(ws_opt.host.to_string(), ws_opt.port, ws_opt.password.as_ref().map(|e| e.expose_secret())).await?;
        info!( "Connected to OBS instance");

        let version = ws.general().version().await?;
        info!("OBS version: {}, platform: {}", version.obs_version, version.platform);

        let hotkey_mgr = GlobalHotKeyManager::new().unwrap();

        let mut plugins = Vec::new();
        let mut hotkey_plugins = HashMap::new();
        let lock = EMBEDDED_PLUGINS.lock().unwrap();
        for (plugin_name, plugin_opts) in plugin_opts.into_iter() {
            debug!("init plugin: {plugin_name}");

            match lock.get(&plugin_name) {
                None => warn!("plugin {} not found", plugin_name),
                Some(f) => {
                    let plugin = f(plugin_opts, &ws)?;

                    if let Some(hotkey) = plugin.hotkey() {
                        hotkey_mgr.register(hotkey)?;
                        hotkey_plugins.insert(hotkey.id(), plugin);
                    } else {
                        plugins.push(plugin);
                    }
                }
            }
        }

        Ok(Self {
            ws,
            plugins,
            hotkey_plugins,

            hotkey_mgr,
        })
    }

    pub fn run(self) -> Result<()> {
        let Self { ws, plugins, mut hotkey_plugins, hotkey_mgr } = self;
        // run background plugins
        let ws = Arc::new(ws);
        for mut plugin in plugins.into_iter() {
            let ws = ws.clone();
            tokio::spawn(async move {
                plugin.run(ws).await.unwrap();
            });
        }

        let event_loop = EventLoopBuilder::new().build();
        let global_hotkey_channel = GlobalHotKeyEvent::receiver();

        // Maybe we can use futures::executor::block_on here?
        // but it seems that sometime main thread will be blocked.
        let event_loop_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        event_loop.run(move |_event, _, control_flow| {
            *control_flow = ControlFlow::Poll;

            if let Ok(event) = global_hotkey_channel.recv() {
                let id = event.id;
                if let Some(plugin) = hotkey_plugins.get_mut(&id) {
                    info!("invoke hotkey plugin: {}", plugin.name());
                    // futures::executor::block_on(async {
                    //    plugin.run(_ws).await.unwarp();
                    // });

                    event_loop_rt.block_on(async {
                        plugin.run(ws.clone()).await.unwrap();
                    });

                    info!("plugin output: {}", plugin.get_output());
                }
            }
        });
    }
}


#[derive(Debug, Deserialize)]
pub(crate) struct WSServer {
    host: IpAddr,
    port: u16,
    password: Option<Secret<String>>,
}

pub(crate) type EmbeddedPluginsItemType = Box<dyn Fn(Table, &obws::Client) -> Result<Box<dyn Plugin>> + Send>;

static EMBEDDED_PLUGINS: Lazy<Mutex<HashMap<String, EmbeddedPluginsItemType>>> = Lazy::new(|| {
    let mut mm: HashMap<String, EmbeddedPluginsItemType> = HashMap::new();
    mm.insert("switch_scene".to_string(), Box::new(
        |config, client| Ok(Box::new(SwitchScenePlugin::new(config, client)?))
    ));

    Mutex::new(mm)
});

#[async_trait]
pub(crate) trait Plugin: Send {
    fn name(&self) -> &str;
    fn hotkey(&self) -> Option<HotKey>;

    fn init_plugin(&mut self, config: Option<Table>, ws: Arc<obws::Client>) -> Result<()>;

    fn get_output(&self) -> &str;

    async fn init(&mut self, ws: Arc<obws::Client>) -> Result<()>;
    async fn run(&mut self, ws: Arc<obws::Client>) -> Result<()>;
}

impl Debug for dyn Plugin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "plugin: {}", self.name())
    }
}

fn parse_hotkey_string<'de, D>(deserializer: D) -> std::result::Result<HotKey, D::Error>
    where D: Deserializer<'de>
{
    let s: String = Deserialize::deserialize(deserializer)?;
    HotKey::from_str(&s).map_err(D::Error::custom)
}

#[derive(Debug, Deserialize)]
pub(crate) struct SwitchScenePlugin {
    #[serde(deserialize_with = "parse_hotkey_string")]
    hotkey: HotKey,
    safe_scene_name: String,

    #[serde(skip)]
    prev_scene_name: Option<String>,

    #[serde(skip)]
    output: String,
}


impl SwitchScenePlugin {
    pub(crate) fn new(config: Table, ws: &obws::Client) -> Result<Self> {
        let ret = Self::deserialize(config.into_deserializer())?;
        Ok(ret)
    }
}

#[async_trait]
impl Plugin for SwitchScenePlugin {
    fn name(&self) -> &str { "switch_scene" }
    fn hotkey(&self) -> Option<HotKey> { Some(self.hotkey) }
    fn init_plugin(&mut self, config: Option<Table>, ws: Arc<obws::Client>) -> Result<()> { unimplemented!() }

    fn get_output(&self) -> &str {
        self.output.as_str()
    }

    async fn init(&mut self, ws: Arc<obws::Client>) -> Result<()> {
        let scene = ws.scenes().current_program_scene().await?;
        self.prev_scene_name = Some(scene);
        Ok(())
    }

    async fn run(&mut self, ws: Arc<obws::Client>) -> Result<()> {
        let scene = ws.scenes().current_program_scene().await?;
        if self.prev_scene_name.is_some() {
            ws.scenes().set_current_program_scene(self.prev_scene_name.as_ref().unwrap()).await?;

            self.output = format!("Switch back from {} to {}",
                                  scene, self.prev_scene_name.as_deref().unwrap_or(""));
            self.prev_scene_name = None;
        } else {
            ws.scenes().set_current_program_scene(&self.safe_scene_name).await?;

            self.output = format!("Active SAFE! from {} to {}", scene, self.safe_scene_name);
            self.prev_scene_name = Some(scene);
        }

        Ok(())
    }
}
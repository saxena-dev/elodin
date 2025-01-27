use crate::*;
use ::s10::{cli::run_recipe, GroupRecipe, SimRecipe};
use clap::Parser;
use impeller2::types::PrimType;
use impeller2_wkt::{ComponentMetadata, EntityMetadata};
use miette::miette;
use nox_ecs::{increment_sim_tick, nox, ComponentSchema, IntoSystem, System as _, TimeStep, World};
use std::{collections::HashMap, iter, net::SocketAddr, path::PathBuf, time};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub enum Args {
    Build {
        #[arg(long)]
        dir: PathBuf,
    },
    Run {
        #[arg(default_value = "0.0.0.0:2240")]
        addr: SocketAddr,
        #[arg(long, default_value = "false")]
        no_s10: bool,
    },
    Plan {
        out_dir: PathBuf,
        #[arg(default_value = "0.0.0.0:2240")]
        addr: SocketAddr,
    },
    #[clap(hide = true)]
    Bench {
        #[arg(long, default_value = "1000")]
        ticks: usize,
    },
}

#[pyclass(subclass)]
#[derive(Default)]
pub struct WorldBuilder {
    pub world: World,
    pub recipes: HashMap<String, ::s10::Recipe>,
}

impl WorldBuilder {
    // fn insert_entity_id(&mut self, archetype: &Archetype, entity_id: EntityId) {
    //     let archetype_name = archetype.archetype_name;
    //     let columns = archetype.component_data.iter().cloned();
    //     for metadata in columns {
    //         let id = metadata.component_id();
    //         self.world
    //             .component_map
    //             .insert(id, (archetype_name, metadata.inner));
    //         self.world.host.entry(id).or_default();
    //     }
    //     // self.world
    //     //     .metadata
    //     //     .entity_ids
    //     //     .entry(archetype_name)
    //     //     .or_default()
    //     //     .extend_from_slice(&entity_id.inner.0.to_le_bytes());
    // }

    fn sim_recipe(&mut self, path: PathBuf, addr: SocketAddr, optimize: bool) -> ::s10::Recipe {
        let sim = SimRecipe {
            path,
            addr,
            optimize,
        };
        let group = GroupRecipe {
            refs: vec![],
            recipes: self
                .recipes
                .iter()
                .map(|(n, r)| (n.clone(), r.clone()))
                .chain(iter::once(("sim".to_string(), ::s10::Recipe::Sim(sim))))
                .collect(),
        };
        ::s10::Recipe::Group(group)
    }
}

#[pymethods]
impl WorldBuilder {
    #[new]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn(&mut self, spawnable: Spawnable, name: Option<String>) -> Result<EntityId, Error> {
        let entity_id = EntityId {
            inner: impeller2::types::EntityId(self.world.entity_len()),
        };
        self.insert(entity_id, spawnable)?;
        self.world.metadata.entity_len += 1;
        if let Some(name) = name {
            self.world.metadata.entity_metadata.insert(
                entity_id.inner,
                EntityMetadata {
                    entity_id: entity_id.inner,
                    name: name.to_string(),
                    metadata: Default::default(),
                },
            );
        }
        Ok(entity_id)
    }

    pub fn insert(&mut self, entity_id: EntityId, spawnable: Spawnable) -> Result<(), Error> {
        match spawnable {
            Spawnable::Archetypes(archetypes) => {
                for archetype in archetypes {
                    for (arr, component) in archetype.arrays.iter().zip(archetype.component_data) {
                        let component_id = ComponentId::new(&component.name);
                        let metadata = ComponentMetadata {
                            component_id,
                            name: component.name.clone().into(),
                            metadata: impeller2_wkt::Metadata {
                                metadata: component.metadata.clone(),
                            },
                            asset: component.asset,
                        };

                        self.world.metadata.component_map.insert(
                            component_id,
                            (ComponentSchema::from(component.clone()), metadata),
                        );
                        let buffer = self.world.host.entry(component_id).or_default();
                        let ty = component.ty.unwrap();
                        let prim_ty: PrimType = ty.ty.into();
                        let size = prim_ty.size();
                        let buf = unsafe { arr.buf(size) };
                        buffer.buffer.extend_from_slice(buf);
                        buffer
                            .entity_ids
                            .extend_from_slice(&entity_id.inner.0.to_le_bytes());
                        self.world.dirty_components.insert(component_id);
                    }
                }
                Ok(())
            }
            Spawnable::Asset { name, bytes } => {
                let name = format!("asset_handle_{name}");
                let component_id = ComponentId::new(&name);
                let metadata = ComponentMetadata {
                    component_id,
                    name: name.into(),
                    metadata: impeller2_wkt::Metadata {
                        metadata: Default::default(),
                    },
                    asset: true,
                };

                self.world.metadata.component_map.insert(
                    component_id,
                    (
                        ComponentSchema {
                            component_id,
                            prim_type: PrimType::U64,
                            shape: iter::empty().collect(),
                            dim: iter::empty().collect(),
                        },
                        metadata,
                    ),
                );
                let inner = self.world.assets.insert_bytes(bytes.bytes);

                let buffer = self.world.host.entry(component_id).or_default();
                buffer.buffer.extend_from_slice(&inner.id.to_le_bytes());
                buffer
                    .entity_ids
                    .extend_from_slice(&entity_id.inner.0.to_le_bytes());

                self.world.dirty_components.insert(component_id);

                Ok(())
            }
        }
    }

    fn insert_asset(&mut self, py: Python<'_>, asset: PyObject) -> Result<Handle, Error> {
        let asset = PyAsset::try_new(py, asset)?;
        let inner = self.world.assets.insert_bytes(asset.bytes()?);
        Ok(Handle { inner })
    }

    fn recipe(&mut self, recipe: crate::s10::Recipe) -> PyResult<()> {
        self.recipes.insert(recipe.name(), recipe.to_rust()?);
        Ok(())
    }

    // #[cfg(feature = "server")]
    // #[pyo3(signature = (
    //     sys,
    //     daemon = False,
    //     sim_time_step = 1.0 / 120.0,
    //     default_playback_speed = 1.0,
    //     max_ticks = None,
    //     addr = "127.0.0.1:0",
    // ))]
    // pub fn serve(
    //     &mut self,
    //     py: Python<'_>,
    //     sys: PyObject,
    //     daemon: bool,
    //     sim_time_step: f64,
    //     default_playback_speed: f64,
    //     max_ticks: Option<u64>,
    //     addr: String,
    // ) -> Result<String, Error> {
    //     use self::web_socket::spawn_ws_server;
    //     use tokio_util::sync::CancellationToken;

    //     let _ = tracing_subscriber::fmt::fmt()
    //         .with_env_filter(
    //             EnvFilter::builder()
    //                 .with_default_directive("info".parse().expect("invalid filter"))
    //                 .from_env_lossy(),
    //         )
    //         .try_init();

    //     let exec =
    //         self.build_uncompiled(py, sys, sim_time_step, default_playback_speed, max_ticks)?;

    //     let client = nox::Client::cpu()?;

    //     let (tx, rx) = flume::unbounded();
    //     if daemon {
    //         let cancel_token = CancellationToken::new();
    //         std::thread::spawn(move || {
    //             spawn_ws_server(
    //                 addr.parse().unwrap(),
    //                 exec,
    //                 &client,
    //                 Some(cancel_token.clone()),
    //                 || cancel_token.is_cancelled(),
    //                 tx,
    //             )
    //             .unwrap();
    //         });
    //     } else {
    //         spawn_ws_server(
    //             addr.parse().unwrap(),
    //             exec,
    //             &client,
    //             None,
    //             || py.check_signals().is_err(),
    //             tx,
    //         )?;
    //     }
    //     Ok(rx.recv().unwrap().to_string())
    // }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        sys,
        sim_time_step = 1.0 / 120.0,
        run_time_step = None,
        default_playback_speed = 1.0,
        max_ticks = None,
        optimize = false,
    ))]
    pub fn run(
        &mut self,
        py: Python<'_>,
        sys: System,
        sim_time_step: f64,
        run_time_step: Option<f64>,
        default_playback_speed: f64,
        max_ticks: Option<u64>,
        optimize: bool,
    ) -> Result<Option<String>, Error> {
        let _ = tracing_subscriber::fmt::fmt()
            .with_env_filter(
                EnvFilter::builder()
                    .with_default_directive("info".parse().expect("invalid filter"))
                    .from_env_lossy(),
            )
            .try_init();

        let args = py
            .import_bound("sys")?
            .getattr("argv")?
            .extract::<Vec<String>>()?;
        let path = args.first().ok_or(Error::MissingArg("path".to_string()))?;
        let path = PathBuf::from(path);
        let args = Args::parse_from(args);

        match args {
            Args::Build { dir } => {
                let mut exec = self.build_uncompiled(
                    py,
                    sys,
                    sim_time_step,
                    run_time_step,
                    default_playback_speed,
                    max_ticks,
                )?;
                exec.write_to_dir(dir)?;
                Ok(None)
            }
            Args::Run { addr, no_s10 } => {
                let exec = self.build_uncompiled(
                    py,
                    sys,
                    sim_time_step,
                    run_time_step,
                    default_playback_speed,
                    max_ticks,
                )?;
                let mut client = nox::Client::cpu()?;
                if !optimize {
                    client.disable_optimizations();
                }
                let recipes = self.recipes.clone();
                if !no_s10 {
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .build()
                            .map_err(|err| miette!("rt err {}", err))
                            .unwrap();
                        let group = ::s10::Recipe::Group(GroupRecipe {
                            recipes,
                            ..Default::default()
                        });
                        rt.block_on(run_recipe("world".to_string(), group, false, false))
                            .unwrap();
                    });
                }
                let exec = exec.compile(client)?;
                py.allow_threads(|| {
                    stellarator::run(|| {
                        let tmpfile = tempfile::tempdir().unwrap().into_path();
                        nox_ecs::impeller2_server::Server::new(
                            elodin_db::Server::new(tmpfile.join("db"), addr).unwrap(),
                            exec,
                        )
                        .run_with_cancellation(|| {
                            Python::with_gil(|py| py.check_signals().is_err())
                        })
                    })?;

                    Ok(None)
                })
            }
            Args::Plan { addr, out_dir } => {
                let recipe = self.sim_recipe(path, addr, optimize);
                let toml = toml::to_string_pretty(&recipe)
                    .map_err(|err| PyValueError::new_err(err.to_string()))?;
                let plan_path = out_dir.join("s10.toml");
                std::fs::write(&plan_path, toml)?;
                Ok(None)
            }
            Args::Bench { ticks } => {
                let mut exec = self.build(
                    py,
                    sys,
                    sim_time_step,
                    run_time_step,
                    default_playback_speed,
                    max_ticks,
                    optimize,
                )?;
                exec.run(py, ticks, true)?;
                let tempdir = tempfile::tempdir()?;
                exec.exec.write_to_dir(tempdir)?;
                let profile = exec.profile();
                println!("copy_to_client time:  {:.3} ms", profile["copy_to_client"]);
                println!("execute_buffers time: {:.3} ms", profile["execute_buffers"]);
                println!("copy_to_host time:    {:.3} ms", profile["copy_to_host"]);
                println!("add_to_history time:  {:.3} ms", profile["add_to_history"]);
                println!("= tick time:          {:.3} ms", profile["tick"]);
                println!("build time:           {:.3} ms", profile["build"]);
                println!("compile time:         {:.3} ms", profile["compile"]);
                println!("write_to_dir time:    {:.3} ms", profile["write_to_dir"]);
                println!("real_time_factor:     {:.3}", profile["real_time_factor"]);
                Ok(None)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        system,
        sim_time_step = 1.0 / 120.0,
        run_time_step = None,
        default_playback_speed = 1.0,
        max_ticks = None,
        optimize = false,
    ))]
    pub fn build(
        &mut self,
        py: Python<'_>,
        system: System,
        sim_time_step: f64,
        run_time_step: Option<f64>,
        default_playback_speed: f64,
        max_ticks: Option<u64>,
        optimize: bool,
    ) -> Result<Exec, Error> {
        let exec = self.build_uncompiled(
            py,
            system,
            sim_time_step,
            run_time_step,
            default_playback_speed,
            max_ticks,
        )?;
        let mut client = nox::Client::cpu()?;
        if !optimize {
            client.disable_optimizations();
        }
        let exec = exec.compile(client.clone())?;
        let db_dir = tempfile::tempdir()?;
        let db_dir = db_dir.into_path();
        Ok(Exec {
            exec,
            db: elodin_db::DB::create(db_dir.join("db"))?,
        })
    }
}

impl WorldBuilder {
    fn build_uncompiled(
        &mut self,
        py: Python<'_>,
        sys: System,
        sim_time_step: f64,
        run_time_step: Option<f64>,
        default_playback_speed: f64,
        max_ticks: Option<u64>,
    ) -> Result<nox_ecs::WorldExec, Error> {
        let mut start = time::Instant::now();
        let ts = time::Duration::from_secs_f64(sim_time_step);
        self.world.metadata.sim_time_step = TimeStep(ts);
        self.world.metadata.run_time_step = TimeStep(ts);
        self.world.metadata.default_playback_speed = default_playback_speed;
        if let Some(ts) = run_time_step {
            let ts = time::Duration::from_secs_f64(ts);
            self.world.metadata.run_time_step = TimeStep(ts);
        }
        if let Some(max_ticks) = max_ticks {
            self.world.metadata.max_tick = max_ticks;
        }

        self.world.set_globals();

        let world = std::mem::take(&mut self.world);
        let xla_exec = increment_sim_tick.pipe(sys).compile(&world).unwrap();
        let tick_exec = xla_exec.compile_hlo_module(py, &world).unwrap();

        let mut exec = nox_ecs::WorldExec::new(world, tick_exec, None);
        exec.profiler.build.observe(&mut start);
        Ok(exec)
    }
}

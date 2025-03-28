use anyhow::anyhow;
use arrow::{
    array::RecordBatch,
    error::ArrowError,
    util::display::{ArrayFormatter, FormatOptions},
};
use impeller2::{
    com_de::Decomponentize,
    schema::Schema,
    table::{Entry, VTableBuilder},
    types::{
        ComponentId, EntityId, Msg, OwnedPacket, OwnedTimeSeries, PacketId, PrimType, Timestamp,
        msg_id,
    },
};

use impeller2::types::{IntoLenPacket, LenPacket};
use impeller2_wkt::*;
use mlua::{AnyUserData, Error, Lua, LuaSerdeExt, MultiValue, ObjectLike, UserData, Value};
use nu_ansi_term::Color;
use rustyline::{
    Completer, CompletionType, Editor, Helper, Hinter, Validator,
    completion::FilenameCompleter,
    highlight::{CmdKind, Highlighter},
    hint::HistoryHinter,
    history::History,
    validate::MatchingBracketValidator,
};
use serde::de::DeserializeOwned;
use std::{
    borrow::Cow::{self, Borrowed, Owned},
    collections::HashMap,
    fmt::Display,
    io::{self, Read, Write, stdout},
    net::ToSocketAddrs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{self, AtomicBool},
    },
    time::Duration,
};
use stellarator::{
    buf::Slice,
    io::{OwnedReader, OwnedWriter, SplitExt},
    net::TcpStream,
};
use zerocopy::{Immutable, IntoBytes, TryFromBytes};

pub use mlua;

pub struct Client {
    rx: impeller2_stella::PacketStream<OwnedReader<TcpStream>>,
    tx: impeller2_stella::PacketSink<OwnedWriter<TcpStream>>,
}

impl Client {
    pub async fn connect<T: ToSocketAddrs>(addr: T) -> anyhow::Result<Self> {
        let addr = addr
            .to_socket_addrs()
            .map_err(anyhow::Error::from)?
            .next()
            .ok_or_else(|| anyhow!("missing socket ip"))?;
        let stream = TcpStream::connect(addr)
            .await
            .map_err(anyhow::Error::from)?;
        let (rx, tx) = stream.split();
        let tx = impeller2_stella::PacketSink::new(tx);
        let rx = impeller2_stella::PacketStream::new(rx);
        Ok(Client { tx, rx })
    }

    pub async fn send_req<M: Msg + DeserializeOwned + Request>(
        &mut self,
        msg: M,
    ) -> anyhow::Result<M::Reply> {
        self.tx.send(&msg).await.0?;
        match self.read_with_error().await? {
            impeller2::types::OwnedPacket::Msg(m) if m.id == M::Reply::ID => {
                let m = m.parse::<M::Reply>().unwrap();
                Ok(m)
            }
            m => Err(anyhow!("wrong msg type {:?}", m)),
        }
    }

    pub async fn read_with_error(&mut self) -> anyhow::Result<OwnedPacket<Slice<Vec<u8>>>> {
        let resp = async {
            let buf = vec![0u8; 1024];
            let pkt = self.rx.next_grow(buf).await?;
            match pkt {
                impeller2::types::OwnedPacket::Msg(m) if m.id == ErrorResponse::ID => {
                    let m = m.parse::<ErrorResponse>()?;
                    Err(anyhow!(m.description))
                }
                pkt => Ok(pkt),
            }
        };
        let timeout = async {
            stellarator::sleep(Duration::from_secs(25)).await;
            Err(anyhow!("request timed out"))
        };
        futures_lite::future::race(timeout, resp).await
    }

    pub async fn get_time_series(
        &mut self,
        lua: &Lua,
        component_id: Value,
        entity_id: u64,
        start: Option<i64>,
        stop: Option<i64>,
    ) -> anyhow::Result<()> {
        let start = start.unwrap_or(i64::MIN);
        let stop = stop.unwrap_or(i64::MAX);
        let id = fastrand::u16(..);

        let component_id: ComponentId = lua.from_value(component_id)?;
        let schema = self.send_req(GetSchema { component_id }).await?;
        let start = Timestamp(start);
        let stop = Timestamp(stop);
        let msg = GetTimeSeries {
            id: id.to_le_bytes(),
            range: start..stop,
            entity_id: EntityId(entity_id),
            component_id,
            limit: Some(256),
        };

        self.tx.send(msg.into_len_packet()).await.0?;
        let pkt = self.read_with_error().await?;
        let time_series = match &pkt {
            impeller2::types::OwnedPacket::TimeSeries(time_series) => time_series,
            _ => return Err(anyhow!("wrong msg type")),
        };

        fn print_time_series_as_table<
            T: Immutable + TryFromBytes + Copy + std::fmt::Display + Default + 'static,
        >(
            time_series: &OwnedTimeSeries<Slice<Vec<u8>>>,
            schema: Schema<Vec<u64>>,
        ) -> Result<(), anyhow::Error> {
            let len = schema.shape().iter().product();
            let data = time_series
                .data()
                .map_err(|err| anyhow!("{err:?} failed to get data"))?;
            let buf = <[T]>::try_ref_from_bytes(data).map_err(|_| anyhow!("failed to get data"))?;
            let mut builder = tabled::builder::Builder::default();
            builder.push_record(["TIME".to_string(), "DATA".to_string()]);
            for (chunk, timestamp) in buf
                .chunks(len)
                .zip(time_series.timestamps().unwrap().iter())
            {
                let view = nox::ArrayView::from_buf_shape_unchecked(chunk, schema.shape());
                let epoch = hifitime::Epoch::from_unix_milliseconds(timestamp.0 as f64 / 1000.0);
                builder.push_record([epoch.to_string(), view.to_string()])
            }
            println!(
                "{}",
                builder
                    .build()
                    .with(tabled::settings::Style::rounded())
                    .with(tabled::settings::style::BorderColor::filled(
                        tabled::settings::Color::FG_BLUE
                    ))
            );
            Ok(())
        }

        let schema = schema.0;
        match schema.prim_type() {
            PrimType::U8 => print_time_series_as_table::<u8>(time_series, schema),
            PrimType::U16 => print_time_series_as_table::<u16>(time_series, schema),
            PrimType::U32 => print_time_series_as_table::<u32>(time_series, schema),
            PrimType::U64 => print_time_series_as_table::<u64>(time_series, schema),
            PrimType::I8 => print_time_series_as_table::<i8>(time_series, schema),
            PrimType::I16 => print_time_series_as_table::<i16>(time_series, schema),
            PrimType::I32 => print_time_series_as_table::<i32>(time_series, schema),
            PrimType::I64 => print_time_series_as_table::<i64>(time_series, schema),
            PrimType::Bool => print_time_series_as_table::<bool>(time_series, schema),
            PrimType::F32 => print_time_series_as_table::<f32>(time_series, schema),
            PrimType::F64 => print_time_series_as_table::<f64>(time_series, schema),
        }
    }

    pub async fn sql(&mut self, sql: &str) -> anyhow::Result<()> {
        let resp = self.send_req(SQLQuery(sql.to_string())).await?;
        let mut decoder = arrow::ipc::reader::StreamDecoder::new();
        let batches = resp
            .batches
            .into_iter()
            .filter_map(|batch| {
                let mut buffer = arrow::buffer::Buffer::from(batch.into_owned());
                decoder.decode(&mut buffer).unwrap()
            })
            .collect::<Vec<_>>();
        let mut table = create_table(&batches, &FormatOptions::default())?;
        println!(
            "{}",
            table.with(tabled::settings::Style::rounded()).with(
                tabled::settings::style::BorderColor::filled(tabled::settings::Color::FG_BLUE)
            )
        );
        Ok(())
    }

    pub async fn send(
        &self,
        lua: &Lua,
        component_id: u64,
        entity_id: u64,
        prim_type: PrimType,
        shape: Vec<u64>,
        buf: Value,
    ) -> anyhow::Result<()> {
        let component_id = ComponentId(component_id);
        let entity_id = EntityId(entity_id);
        let mut vtable: VTableBuilder<Vec<_>, Vec<_>> = VTableBuilder::default();
        vtable.column(
            component_id,
            prim_type,
            shape.into_iter(),
            std::iter::once(entity_id),
        )?;
        let vtable = vtable.build();
        let id: [u8; 2] = fastrand::u16(..).to_le_bytes();
        let msg = VTableMsg { id, vtable };
        self.tx.send(msg.into_len_packet()).await.0?;
        let mut table = LenPacket::table(id, 8);
        match prim_type {
            PrimType::U8 => {
                let buf: Vec<u8> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::U16 => {
                let buf: Vec<u16> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::U32 => {
                let buf: Vec<u32> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::U64 => {
                let buf: Vec<u64> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::I8 => {
                let buf: Vec<i8> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::I16 => {
                let buf: Vec<i16> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::I32 => {
                let buf: Vec<i32> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::I64 => {
                let buf: Vec<i64> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::Bool => {
                let buf: Vec<bool> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::F32 => {
                let buf: Vec<f32> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
            PrimType::F64 => {
                let buf: Vec<f64> = lua.from_value(buf)?;
                let buf = buf.as_bytes();
                table.extend_from_slice(buf);
            }
        }
        self.tx.send(table).await.0?;
        Ok(())
    }

    pub async fn stream(&mut self, mut stream: Stream) -> anyhow::Result<()> {
        if stream.id == 0 {
            stream.id = fastrand::u64(..);
        }
        self.tx.send(stream.into_len_packet()).await.0?;
        let mut vtable = HashMap::new();
        let mut buf = vec![0; 1024 * 8];
        let cancel = Arc::new(AtomicBool::new(true));
        let canceler = cancel.clone();
        std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut buf = [0u8];
            let _ = stdin.read(&mut buf);
            canceler.store(false, atomic::Ordering::SeqCst);
        });

        while cancel.load(atomic::Ordering::SeqCst) {
            let pkt = self.rx.next(buf).await?;
            match &pkt {
                impeller2::types::OwnedPacket::Msg(msg) if msg.id == VTableMsg::ID => {
                    let msg = msg.parse::<VTableMsg>()?;
                    vtable.insert(msg.id, msg.vtable);
                }
                impeller2::types::OwnedPacket::Msg(msg) => {
                    println!("msg ({:?}) = {:?}", msg.id, &msg.buf[..]);
                }
                impeller2::types::OwnedPacket::Table(table) => {
                    if let Some(vtable) = vtable.get(&table.id) {
                        vtable.parse_table(&table.buf[..], &mut DebugSink)?;
                    } else {
                        println!("table ({:?}) = {:?}", table.id, &table.buf[..]);
                    }
                }
                impeller2::types::OwnedPacket::TimeSeries(_) => {}
            }
            buf = pkt.into_buf().into_inner();
        }
        Ok(())
    }

    pub async fn stream_msgs(&mut self, stream_msgs: MsgStream) -> anyhow::Result<()> {
        let request_id = fastrand::u8(..);
        let metadata = self
            .send_req(GetMsgMetadata {
                msg_id: stream_msgs.msg_id,
            })
            .await?;
        self.tx
            .send(stream_msgs.with_request_id(request_id))
            .await
            .0?;

        let mut buf = vec![0; 1024 * 8];
        let cancel = Arc::new(AtomicBool::new(true));
        let canceler = cancel.clone();
        std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut buf = [0u8];
            let _ = stdin.read(&mut buf);
            canceler.store(false, atomic::Ordering::SeqCst);
        });

        while cancel.load(atomic::Ordering::SeqCst) {
            let pkt = self.rx.next(buf).await?;
            match &pkt {
                impeller2::types::OwnedPacket::Msg(msg) if msg.req_id == request_id => {
                    let data = postcard_dyn::from_slice_dyn(&metadata.schema, &msg.buf[..])
                        .map_err(|e| anyhow!("failed to deserialize msg: {:?}", e))?;
                    println!("{:?}", data);
                }
                _ => {}
            }
            buf = pkt.into_buf().into_inner();
        }
        Ok(())
    }

    pub async fn get_msgs(
        &mut self,
        msg_id: PacketId,
        start: Option<i64>,
        stop: Option<i64>,
    ) -> anyhow::Result<()> {
        let start = Timestamp(start.unwrap_or(i64::MIN));
        let stop = Timestamp(stop.unwrap_or(i64::MAX));
        let metadata = self.send_req(GetMsgMetadata { msg_id }).await?;
        let get_msgs = GetMsgs {
            msg_id,
            range: start..stop,
            limit: Some(1000),
        };
        let batch = self.send_req(get_msgs).await?;
        let mut builder = tabled::builder::Builder::default();
        for (timestamp, msg) in batch.data {
            let data = postcard_dyn::from_slice_dyn(&metadata.schema, &msg[..])
                .map_err(|e| anyhow!("failed to deserialize msg: {:?}", e))?;

            let epoch = hifitime::Epoch::from_unix_milliseconds(timestamp.0 as f64 / 1000.0);
            builder.push_record([epoch.to_string(), data.to_string()]);
        }
        println!(
            "{}",
            builder
                .build()
                .with(tabled::settings::Style::rounded())
                .with(tabled::settings::style::BorderColor::filled(
                    tabled::settings::Color::FG_BLUE
                ))
        );
        Ok(())
    }

    pub async fn send_msg(
        &mut self,
        msg_id: PacketId,
        msg: postcard_dyn::Value,
    ) -> anyhow::Result<()> {
        let metadata = self.send_req(GetMsgMetadata { msg_id }).await?;
        let bytes =
            postcard_dyn::to_stdvec_dyn(&metadata.schema, &msg).map_err(|e| anyhow!("{e:?}"))?;
        let mut pkt = LenPacket::msg(msg_id, bytes.len());
        pkt.extend_from_slice(&bytes);
        self.tx.send(pkt).await.0?;
        Ok(())
    }
}

fn create_table(
    results: &[RecordBatch],
    options: &FormatOptions,
) -> anyhow::Result<tabled::Table, anyhow::Error> {
    let mut builder = tabled::builder::Builder::default();

    if results.is_empty() {
        return Ok(builder.build());
    }

    let schema = results[0].schema();

    let mut header = Vec::new();
    for field in schema.fields() {
        header.push(field.name());
    }
    builder.push_record(header);

    for batch in results {
        let formatters = batch
            .columns()
            .iter()
            .map(|c| ArrayFormatter::try_new(c.as_ref(), options))
            .collect::<Result<Vec<_>, ArrowError>>()?;

        for row in 0..batch.num_rows() {
            let mut cells = Vec::new();
            for formatter in &formatters {
                cells.push(formatter.value(row).to_string());
            }
            builder.push_record(cells);
        }
    }

    Ok(builder.build())
}

impl UserData for Client {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method(
            "send_table",
            |lua, this, (component_id, entity_id, ty, shape, buf): (Value, _, _, Vec<u64>, _)| async move {

                    let component_id = if let Ok(id) = lua.from_value::<ComponentId>(component_id.clone()) {
                        id
                    } else if let Ok(name) = lua.from_value::<String>(component_id.clone()) {
                        ComponentId::new(&name)
                    } else if let Ok(id) = lua.from_value::<i64>(component_id) {
                        ComponentId(id as u64)
                    } else {
                        return Err(anyhow!("msg id must be a PacketId or String").into());
                    };
                let ty: PrimType = lua.from_value(ty)?;
                this.send(&lua, component_id.0, entity_id, ty, shape, buf).await?;
                Ok(())
            },
        );
        methods.add_async_method_mut(
            "send_msg",
            |lua, mut this, (msg_or_id, val): (Value, Option<Value>)| async move {
                if let Some(msg) = msg_or_id.as_userdata() {
                    let msg = msg.call_method::<Vec<u8>>("msg", ())?;
                    this.tx
                        .send(LenPacket { inner: msg })
                        .await
                        .0
                        .map_err(anyhow::Error::from)?;
                } else if let Some(msg) = val {
                    let id = msg_or_id;
                    let msg_id = if let Ok(id) = lua.from_value::<PacketId>(id.clone()) {
                        id
                    } else if let Ok(name) = lua.from_value::<String>(id) {
                        msg_id(&name)
                    } else {
                        return Err(anyhow!("msg id must be a PacketId or String").into());
                    };
                    let msg = lua.from_value(msg)?;
                    this.send_msg(msg_id, msg).await?;
                } else {
                    return Err(anyhow!(
                        "send_msg requires either a native msg or a id and a table"
                    )
                    .into());
                };
                Ok(())
            },
        );

        methods.add_async_method(
            "send_msgs",
            |_lua, this, msgs: Vec<AnyUserData>| async move {
                for msg in msgs {
                    let msg = msg.call_method::<Vec<u8>>("msg", ())?;
                    this.tx
                        .send(LenPacket { inner: msg })
                        .await
                        .0
                        .map_err(anyhow::Error::from)?;
                }
                Ok(())
            },
        );

        methods.add_async_method_mut("sql", |_lua, mut this, sql: String| async move {
            this.sql(&sql).await?;
            Ok(())
        });
        methods.add_async_method_mut(
            "get_time_series",
            |lua, mut this, (c_id, e_id, start, stop)| async move {
                this.get_time_series(&lua, c_id, e_id, start, stop).await?;
                Ok(())
            },
        );
        methods.add_async_method_mut("stream", |lua, mut this, stream| async move {
            let msg: Stream = lua.from_value(stream)?;
            this.stream(msg).await?;
            Ok(())
        });

        methods.add_async_method_mut("stream_msgs", |lua, mut this, id: Value| async move {
            let msg_id = if let Ok(id) = lua.from_value::<PacketId>(id.clone()) {
                id
            } else if let Ok(name) = lua.from_value::<String>(id) {
                msg_id(&name)
            } else {
                return Err(anyhow!("msg id must be a PacketId or String").into());
            };
            this.stream_msgs(MsgStream { msg_id }).await?;
            Ok(())
        });

        methods.add_async_method_mut(
            "get_msgs",
            |lua, mut this, (id, start, stop): (Value, Option<i64>, Option<i64>)| async move {
                let msg_id = if let Ok(id) = lua.from_value::<PacketId>(id.clone()) {
                    id
                } else if let Ok(name) = lua.from_value::<String>(id) {
                    msg_id(&name)
                } else {
                    return Err(anyhow!("msg id must be a PacketId or String").into());
                };
                this.get_msgs(msg_id, start, stop).await?;
                Ok(())
            },
        );

        macro_rules! add_req_reply_method {
            ($name:tt, $ty:tt, $req:tt) => {
                methods.add_async_method_mut(
                    stringify!($name),
                    |lua, mut this, value| async move {
                        let msg: $ty = lua.from_value(value)?;
                        let res = this.send_req(msg).await?;
                        lua.to_value(&res)
                    },
                );
            };
        }
        add_req_reply_method!(get_asset, GetAsset, Asset);
        add_req_reply_method!(
            get_component_metadata,
            GetComponentMetadata,
            ComponentMetadata
        );
        add_req_reply_method!(dump_metadata, DumpMetadata, DumpMetadataResp);
        add_req_reply_method!(get_entity_metadata, GetEntityMetadata, EntityMetadata);
        add_req_reply_method!(get_schema, GetSchema, SchemaMsg);
    }
}

struct LuaVTableBuilder {
    id: PacketId,
    vtable: impeller2::table::VTableBuilder<Vec<Entry>, Vec<u8>>,
}

impl UserData for LuaVTableBuilder {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut(
            "column",
            |lua,
             this,
             (component_id, prim_type, shape, entity_ids): (
                mlua::Value,
                mlua::Value,
                mlua::Value,
                mlua::Value,
            )| {
                let component_id: ComponentId = lua.from_value(component_id)?;
                let prim_type: PrimType = lua.from_value(prim_type)?;
                let shape: Vec<u64> = lua.from_value(shape)?;
                let entity_ids: Vec<EntityId> = lua.from_value(entity_ids)?;
                let _ = this
                    .vtable
                    .column(component_id, prim_type, shape, entity_ids);
                Ok(())
            },
        );
        methods.add_method("msg", |_, this, ()| {
            let vtable_msg = VTableMsg {
                id: this.id,
                vtable: this.vtable.clone().build(),
            };
            let vtable_msg = vtable_msg.into_len_packet().inner;
            Ok(vtable_msg)
        });
        methods.add_method("build", |_, this, ()| Ok(this.build()));
        methods.add_method("build_bin", |_, this, ()| {
            let stdout = stdout();
            let bytes = this.build();
            let mut stdout = stdout.lock();
            stdout.write_all(&bytes)?;
            Ok(())
        });
    }
}

impl LuaVTableBuilder {
    pub fn new(id: PacketId) -> Self {
        Self {
            id,
            vtable: Default::default(),
        }
    }
    pub fn build(&self) -> Vec<u8> {
        let vtable = VTableMsg {
            id: self.id,
            vtable: self.vtable.clone().build(),
        };
        postcard::to_allocvec(&vtable).expect("vtable build failed")
    }
}

#[derive(Helper, Completer, Validator, Hinter)]
struct CliHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
    #[rustyline(Validator)]
    validator: MatchingBracketValidator,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
}

impl Highlighter for CliHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Owned(Color::Blue.bold().paint(prompt).to_string())
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned(Color::Default.dimmed().paint(hint).to_string())
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        #[cfg(feature = "highlight")]
        let out = syntastica::highlight(
            line,
            syntastica_parsers::Lang::Lua,
            &syntastica_parsers::LanguageSetImpl::new(),
            &mut syntastica::renderer::TerminalRenderer::new(None),
            syntastica_themes::catppuccin::mocha(),
        )
        .unwrap()
        .into();
        #[cfg(not(feature = "highlight"))]
        let out = Cow::Borrowed(line);
        out
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        false
    }
}

#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    pub path: Option<PathBuf>,
}

struct LuaMsg<M: Msg>(M);

impl<M: Msg> UserData for LuaMsg<M> {
    fn add_methods<T: mlua::UserDataMethods<Self>>(methods: &mut T) {
        methods.add_method("msg", |_, this, ()| {
            let msg = this.0.into_len_packet().inner;
            Ok(msg)
        });
    }
}

pub fn lua() -> anyhow::Result<Lua> {
    let lua = Lua::new();
    let client = lua.create_async_function(|_lua, addr: String| async move {
        let c = Client::connect(addr).await?;
        Ok(c)
    })?;
    lua.globals().set(
        "VTableBuilder",
        lua.create_function(|_, id: u16| Ok(LuaVTableBuilder::new(id.to_le_bytes())))?,
    )?;
    lua.globals().set("connect", client)?;
    lua.globals().set(
        "ComponentId",
        lua.create_function(|lua, name: String| lua.create_ser_userdata(ComponentId::new(&name)))?,
    )?;
    lua.globals().set(
        "SetComponentMetadata",
        lua.create_function(|lua, m: SetComponentMetadata| lua.create_ser_userdata(m))?,
    )?;
    lua.globals().set(
        "SetEntityMetadata",
        lua.create_function(|lua, m: SetEntityMetadata| lua.create_ser_userdata(m))?,
    )?;
    lua.globals().set(
        "Stream",
        lua.create_function(|lua, m: Stream| lua.create_ser_userdata(m))?,
    )?;
    lua.globals().set(
        "UdpUnicast",
        lua.create_function(|lua, m: UdpUnicast| lua.create_ser_userdata(m))?,
    )?;

    lua.globals().set(
        "SQLQuery",
        lua.create_function(|lua, m: SQLQuery| lua.create_ser_userdata(m))?,
    )?;
    Ok(lua)
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    let lua = lua()?;
    if let Some(path) = args.path {
        let script = std::fs::read_to_string(path)?;
        lua.load(&script).eval_async::<MultiValue>().await?;
        Ok(())
    } else {
        let config = rustyline::Config::builder()
            .history_ignore_space(true)
            .completion_type(CompletionType::List)
            .auto_add_history(true)
            .build();
        let h = CliHelper {
            completer: FilenameCompleter::new(),
            hinter: HistoryHinter::new(),
            validator: MatchingBracketValidator::new(),
        };
        let mut history = rustyline::history::FileHistory::with_config(config);
        let dirs = directories::ProjectDirs::from("systems", "elodin", "impeller2-cli")
            .ok_or_else(|| anyhow!("dir not found"))?;
        std::fs::create_dir_all(dirs.data_dir())?;
        let history_path = dirs.data_dir().join("impeller2-history");
        if history_path.exists() {
            history.load(&history_path)?;
        }
        let mut editor: Editor<_, _> = Editor::with_history(config, history)?;
        editor.set_helper(Some(h));

        let mut mode = Mode::Lua;
        loop {
            let mut prompt = match &mode {
                Mode::Lua => "db ❯❯ ",
                Mode::Sql(..) => "sql ❯❯ ",
            };
            let mut line = String::new();
            loop {
                line.clear();
                match editor.readline(prompt) {
                    Ok(input) => line.push_str(&input),
                    Err(_) => return Ok(()),
                }

                if line == ":exit" {
                    if matches!(mode, Mode::Sql(_)) {
                        mode = Mode::Lua;
                        break;
                    }
                    std::process::exit(0);
                }
                if line.starts_with(":sql") {
                    let addr = &line.strip_prefix(":sql ").unwrap_or_default();
                    let addr = if addr.is_empty() {
                        "localhost:2240"
                    } else {
                        addr
                    };
                    let client = match Client::connect(addr).await {
                        Ok(c) => c,
                        Err(err) => {
                            println!("{err}");
                            continue;
                        }
                    };
                    mode = Mode::Sql(client);
                    break;
                }
                if line == ":help" || line == ":h" {
                    println!("{}", Color::Yellow.bold().paint("Impeller Lua REPL"));
                    print_usage_line(
                        ":sql addr",
                        "Connects to a database and drops you into a sql repl",
                    );
                    print_usage_line(
                        "connect(addr) -> Client",
                        "Connects to a database and returns a client",
                    );
                    print_usage_line(
                        "Client:send_table(component_id, entity_id, ty, shape, data)",
                        "Sends a new ComponentValue to the db",
                    );
                    print_usage_line("Client:send_msg(msg)", "Sends a raw message to the db");
                    print_usage_line(
                        "Client:send_msgs(msgs)",
                        "Sends a list of raw messages to the db",
                    );
                    print_usage_line(
                        "Client:get_component_metadata(GetComponentMetadata)",
                        format!(
                            "Gets a component's metadata using {} {{ id }}",
                            Color::Blue.bold().paint("GetComponentMetadata")
                        ),
                    );
                    print_usage_line(
                        "Client:get_entity_metadata(GetEntityMetadata)",
                        format!(
                            "Gets a entity's metadata using {} {{ id }}",
                            Color::Blue.bold().paint("GetEntityMetadata")
                        ),
                    );
                    print_usage_line(
                        "Client:get_asset(GetAsset)",
                        format!(
                            "Gets a entity's metadata using {} {{ id }}",
                            Color::Blue.bold().paint("GetAsset")
                        ),
                    );
                    print_usage_line("Client:dump_metadata()", "Dumps all metadata from the db ");
                    print_usage_line(
                        "Client:get_schema(GetSchema)",
                        format!(
                            "Gets a components schema {} {{ id }}",
                            Color::Blue.bold().paint("GetSchema")
                        ),
                    );
                    println!("{}", Color::Yellow.bold().paint("Messages"));
                    print_message("SetComponentMetadata { component_id, name, metadata, asset }");
                    print_message("SetEntityMetadata { entity_id, name, metadata }");
                    print_message(
                        "UdpUnicast { stream = { filter = { component_id, entity_id }, id }, port }",
                    );
                    print_message("SetStreamState { id, playing, tick, time_step }");
                    print_message("SetAsset { id, buf }");
                    break;
                }
                editor.save_history(&history_path)?;
                editor.add_history_entry(line.clone())?;
                match &mut mode {
                    Mode::Sql(client) => {
                        if line.is_empty() {
                            continue;
                        }
                        if let Err(err) = client.sql(&line).await {
                            let err = err.to_string();
                            println!("{}", Color::Red.paint(&err));
                        }
                    }
                    Mode::Lua => match lua.load(&line).eval_async::<MultiValue>().await {
                        Ok(values) => {
                            println!(
                                "{}",
                                values
                                    .iter()
                                    .map(|value| {
                                        #[cfg(not(feature = "highlight"))]
                                        let out = format!("{:#?}", value);
                                        #[cfg(feature = "highlight")]
                                        let out = syntastica::highlight(
                                            format!("{:#?}", value),
                                            syntastica_parsers::Lang::Lua,
                                            &syntastica_parsers::LanguageSetImpl::new(),
                                            &mut syntastica::renderer::TerminalRenderer::new(None),
                                            syntastica_themes::catppuccin::mocha(),
                                        )
                                        .unwrap()
                                        .to_string();
                                        out
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\t")
                            );
                            break;
                        }
                        Err(Error::SyntaxError {
                            incomplete_input: true,
                            ..
                        }) => {
                            line.push('\n');
                            prompt = ">> ";
                        }
                        Err(e) => {
                            let err = e.to_string();
                            let err = Color::Red.paint(&err);
                            eprintln!("{}", err);
                            break;
                        }
                    },
                }
            }
        }
    }
}

enum Mode {
    Lua,
    Sql(Client),
}

fn print_usage_line(name: impl Display, desc: impl Display) {
    let name = Color::Green.bold().paint(format!("- `{name}`")).to_string();
    println!("{name}");
    println!("   {desc}",);
}

fn print_message(msg: impl Display) {
    let msg = Color::Green.bold().paint(format!("- `{msg}`")).to_string();
    println!("{msg}");
}

struct DebugSink;

impl Decomponentize for DebugSink {
    fn apply_value(
        &mut self,
        component_id: ComponentId,
        entity_id: EntityId,
        value: impeller2::types::ComponentView<'_>,
        timestamp: Option<Timestamp>,
    ) {
        let epoch = timestamp
            .map(|timestamp| hifitime::Epoch::from_unix_milliseconds(timestamp.0 as f64 / 1000.0));
        println!("({component_id:?},{entity_id:?}) @ {epoch:?} = {value:?}");
    }
}

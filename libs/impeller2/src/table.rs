//! Tables are one of `impellers` two fundamental data types. They represent data that can be tagged with entity_ids and component_ids.
//!
//! A table is essentially a pile of n-dimensional row-major arrays stored in little-endian. Each set of arrays is packed together, with padding in-between to ensure they are aligned.
//! On its own that isn't very useful -- The table itself does not contain entity_ids, component_ids, or even information about where on array starts or stops. In other words,
//! it is a non-self-describing serialization format.
//!
//! To parse a table you need a [`VTable`]; VTable's contain a description of the contents of the table. At a high level they are a list of offsets and shapes to arrays.
//! A VTable is made's contain a description of the contents of the table. At a high level they are a list of offsets and shapes to arrays. Each VTable is made up of a series of entries: [`Entry`].
//! There are three types of entries: [`ColumnEntry`], [`EntityEntry`], [`MetadataEntry`]. Each entry contains an offset to a section of data in the actual table. After the entries the [`VTable`] contains
//! an auxiliary data section. Entries use the data section to store variable length data such as array shapes or lists of component_ids.
//!
//! You can imagine an ECS system as a table with each column being a single component, and each row being a single entity
//!
//! | Entity | Component A (f2) | Component B (u8) |
//! | 1      | 1.0         | 2         |
//! | 2      | 64.0        | 4         |
//!
//! [`ColumnEntry`]s point to a single column in the above table. In other words they point to a uniform series of component arrays. Each array is associated with a single entity. Entity IDs are stored in the VTable's in the auxiliary data section.
//!
//! If you had a column entry for `Component B`, the data would look like `[0x02, 0x04]`
//!
//! [`RowEntry`]s point to a row in the above table. Each separate component array is stored sequentially. The shapes, and component_ids are all stored in the VTable's data section.
//!
//! If you had a row entry for `Entity 1`, the data would look like `[0x0, 0x0, 0x80, 0x3F, 0x02]`. Which is the concatenation of the little-endian bytes for `1.0f32` and `2u8`
//!
//! [`MetadataEntry`]s point to a piece of metadata. This is broadly similar to the header fields in an HTTP request. It is usually used to tag the data with out-of-band information,
//! like the current time or origin of the data.

use crate::buf::{Buf, ByteBufExt};
use core::{marker::PhantomData, mem};
use serde::{Deserialize, Serialize};
use zerocopy::{FromBytes, Immutable, IntoBytes, TryFromBytes};

use crate::{
    com_de::Decomponentize,
    error::Error,
    types::{ComponentId, ComponentView, EntityId, PrimType, Timestamp},
};

/// An entry that points to a series of arrays that are all associated with a single component
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ColumnEntry {
    len: u64,
    component_id: ComponentId,
    entity_ids_entry: BufEntry<EntityId>,
    data_col_offset: u64,
    shape_entry: ShapeEntry,
    timestamp_offset: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct BufEntry<T> {
    offset: u64,
    phantom_data: PhantomData<T>,
}

impl<T> Clone for BufEntry<T> {
    fn clone(&self) -> Self {
        Self {
            offset: self.offset,
            phantom_data: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for BufEntry<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufEntry")
            .field("offset", &self.offset)
            .finish()
    }
}

impl<T: Immutable + TryFromBytes> BufEntry<T> {
    pub fn parse<'a>(&self, vdata: &'a [u8], len: usize) -> Result<&'a [T], Error> {
        let offset: usize = self.offset.try_into().map_err(|_| Error::OffsetOverflow)?;
        let vdata = vdata.get(offset..).ok_or(Error::BufferUnderflow)?;
        <[T]>::try_ref_from_prefix_with_elems(vdata, len)
            .map_err(Error::from)
            .map(|(data, _)| data)
    }
}

#[derive(Serialize, Deserialize, TryFromBytes, Immutable, IntoBytes, Debug, Clone)]
pub(crate) struct ShapeEntry {
    pub(crate) prim_type: PrimType,
    pub(crate) rank: u64,
    pub(crate) shape_offset: u64,
}

impl ShapeEntry {
    pub fn parse_shape<'a>(&'a self, vdata: &'a [u8]) -> Result<&'a [usize], Error> {
        let shape_offset: usize = self
            .shape_offset
            .try_into()
            .map_err(|_| Error::OffsetOverflow)?;
        let rank: usize = self.rank.try_into().map_err(|_| Error::OffsetOverflow)?;
        let data = vdata.get(shape_offset..).ok_or(Error::BufferUnderflow)?;
        <[usize]>::ref_from_prefix_with_elems(data, rank)
            .map_err(Error::from)
            .map(|(shape, _)| shape)
    }
}

impl ColumnEntry {
    pub fn parse_table(
        &self,
        data: &[u8],
        table: &[u8],
        sink: &mut impl Decomponentize,
    ) -> Result<(), Error> {
        let shape = self.shape_entry.parse_shape(data)?;
        let arr_len = arr_len(shape)?;
        let arr_size = arr_len * self.shape_entry.prim_type.size();
        let len = self.len as usize;
        let entity_ids = self.entity_ids_entry.parse(data, len)?;
        let timestamp = if let Some(offset) = self.timestamp_offset {
            let offset: usize = offset.try_into().map_err(|_| Error::OffsetOverflow)?;
            let end = offset
                .checked_add(size_of::<Timestamp>())
                .ok_or(Error::OffsetOverflow)?;
            let timestamp = table.get(offset..end).ok_or(Error::BufferUnderflow)?;
            let timestamp = Timestamp::read_from_bytes(timestamp)?;
            Some(timestamp)
        } else {
            None
        };
        for (i, entity_id) in entity_ids.iter().enumerate() {
            let arr_offset = i * arr_size + self.data_col_offset as usize;
            let arr_data = table.get(arr_offset..).ok_or(Error::BufferUnderflow)?;
            let view =
                ComponentView::try_from_bytes_shape(arr_data, shape, self.shape_entry.prim_type)?;
            sink.apply_value(self.component_id, *entity_id, view, timestamp);
        }
        Ok(())
    }
}

/// An entry that points to a single entity's components. It points to a contiguous series of component arrays. The associated components_ids and shapes are all stored in the [`VTable`]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RowEntry {
    len: u64,
    entity_id: EntityId,
    component_ids_entry: BufEntry<ComponentId>,
    shapes_entry: BufEntry<ShapeEntry>,
    data_col_offset: u64,
    timestamp_offset: Option<u64>,
}

impl RowEntry {
    pub fn parse_table(
        &self,
        vdata: &[u8],
        table: &[u8],
        sink: &mut impl Decomponentize,
    ) -> Result<(), Error> {
        let len: usize = self.len.try_into().map_err(|_| Error::OffsetOverflow)?;
        let component_ids = self.component_ids_entry.parse(vdata, len)?;
        let shapes = self.shapes_entry.parse(vdata, len)?;
        let mut arr_offset = self.data_col_offset as usize; // NOTE(sphw): we are assuming packed values here, but we might want to eventually allow for a list of offsets instead
        let timestamp = if let Some(offset) = self.timestamp_offset {
            let offset: usize = offset.try_into().map_err(|_| Error::OffsetOverflow)?;
            let end = offset
                .checked_add(size_of::<Timestamp>())
                .ok_or(Error::OffsetOverflow)?;
            let timestamp = table.get(offset..end).ok_or(Error::BufferUnderflow)?;
            let timestamp = Timestamp::read_from_bytes(timestamp)?;
            Some(timestamp)
        } else {
            None
        };
        for (component_id, shape_entry) in component_ids.iter().zip(shapes.iter()) {
            let shape = shape_entry.parse_shape(vdata)?;
            let arr_len = arr_len(shape)?;
            let arr_size = arr_len * shape_entry.prim_type.size();
            let arr_data = table.get(arr_offset..).ok_or(Error::BufferUnderflow)?;
            let view = ComponentView::try_from_bytes_shape(arr_data, shape, shape_entry.prim_type)?;
            sink.apply_value(*component_id, self.entity_id, view, timestamp);
            arr_offset += arr_size;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetadataEntry {
    id: u64,
    len: u64,
    offset: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Entry {
    Column(ColumnEntry),
    Entity(RowEntry),
    Metadata(MetadataEntry),
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct VTable<EntryBuf: Buf<Entry>, DataBuf: Buf<u8>> {
    #[serde(bound(deserialize = ""))]
    pub entries: EntryBuf,
    #[serde(bound(deserialize = ""))]
    data: DataBuf,
}

impl Clone for VTable<Vec<Entry>, Vec<u8>> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            data: self.data.clone(),
        }
    }
}

impl<EntryBuf: Buf<Entry>, DataBuf: Buf<u8>> VTable<EntryBuf, DataBuf> {
    pub fn parse_table(&self, table: &[u8], sink: &mut impl Decomponentize) -> Result<(), Error> {
        let data = self.data.as_slice();
        for entry in self.entries.iter() {
            match entry {
                Entry::Column(col) => col.parse_table(data, table, sink)?,
                Entry::Entity(entity) => entity.parse_table(data, table, sink)?,
                Entry::Metadata(_) => {}
            }
        }
        Ok(())
    }

    pub fn parse_metadata<'a>(&'a self, table: &'a [u8]) -> impl Iterator<Item = (u64, &'a [u8])> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Metadata(m) => {
                let offset: usize = m.offset.try_into().ok()?;
                let len: usize = m.len.try_into().ok()?;
                Some((m.id, table.get(offset..offset + len)?))
            }
            _ => None,
        })
    }

    pub fn column_iter(
        &self,
    ) -> impl Iterator<Item = (EntityId, ComponentId, PrimType, &[usize])> + '_ {
        let columns = self
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Column(e) => {
                    let prim_type = e.shape_entry.prim_type;
                    let shape = e.shape_entry.parse_shape(self.data.as_slice()).ok()?;
                    let ids = e
                        .entity_ids_entry
                        .parse(self.data.as_slice(), e.len as usize)
                        .ok()?;
                    Some((e.component_id, prim_type, shape, ids))
                }
                _ => None,
            })
            .flat_map(|(component_id, prim_type, shape, ids)| {
                ids.iter()
                    .copied()
                    .map(move |entity_id| (entity_id, component_id, prim_type, shape))
            });
        let entities = self
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Entity(e) => {
                    let shape_entries = e
                        .shapes_entry
                        .parse(self.data.as_slice(), e.len as usize)
                        .ok()?;
                    let ids = e
                        .component_ids_entry
                        .parse(self.data.as_slice(), e.len as usize)
                        .ok()?;
                    Some((e.entity_id, ids, shape_entries))
                }
                _ => None,
            })
            .flat_map(|(entity_id, ids, shape_entries)| {
                let prim_types = shape_entries.iter().map(|s| s.prim_type);
                let shapes = shape_entries
                    .iter()
                    .map(|s| s.parse_shape(self.data.as_slice()).ok().unwrap());
                ids.iter().copied().zip(prim_types).zip(shapes).map(
                    move |((component_id, prim_type), shape)| {
                        (entity_id, component_id, prim_type, shape)
                    },
                )
            });
        columns.chain(entities)
    }
}

#[derive(Default)]
pub struct VTableBuilder<EntryBuf: Buf<Entry>, DataBuf: Buf<u8>> {
    vtable: VTable<EntryBuf, DataBuf>,
    data_len: usize,
}

impl Clone for VTableBuilder<Vec<Entry>, Vec<u8>> {
    fn clone(&self) -> Self {
        Self {
            vtable: self.vtable.clone(),
            data_len: self.data_len,
        }
    }
}

impl<EntryBuf: Buf<Entry>, DataBuf: Buf<u8>> VTableBuilder<EntryBuf, DataBuf> {
    pub fn build(self) -> VTable<EntryBuf, DataBuf> {
        self.vtable
    }

    pub fn timestamp(&mut self) -> Result<u64, Error> {
        self.data_len += PrimType::I64.padding(self.data_len);
        let offset = self.data_len;
        self.data_len += size_of::<i64>();
        Ok(offset as u64)
    }

    pub fn column<I: IntoIterator<Item = EntityId>, S: IntoIterator<Item = u64>>(
        &mut self,
        component_id: impl Into<ComponentId>,
        prim_type: PrimType,
        shape: S,
        entity_ids: I,
    ) -> Result<&mut Self, Error>
    where
        I::IntoIter: ExactSizeIterator,
    {
        self.column_with_timestamp(component_id, prim_type, shape, entity_ids, None)
    }

    pub fn column_with_timestamp<I: IntoIterator<Item = EntityId>, S: IntoIterator<Item = u64>>(
        &mut self,
        component_id: impl Into<ComponentId>,
        prim_type: PrimType,
        shape: S,
        entity_ids: I,
        timestamp_offset: Option<u64>,
    ) -> Result<&mut Self, Error>
    where
        I::IntoIter: ExactSizeIterator,
    {
        let shape = shape.into_iter();
        let entity_ids = entity_ids.into_iter();
        let component_id = component_id.into();
        let len = entity_ids.len();
        let mut rank: u64 = 0;
        let mut arr_len: usize = 1;
        self.vtable.data.pad_for_alignment::<u64>()?;
        let shape_offset = self.vtable.data.as_slice().len() as u64;
        for d in shape {
            rank = rank.checked_add(1).ok_or(Error::OffsetOverflow)?;
            arr_len = arr_len
                .checked_mul(d as usize)
                .ok_or(Error::OffsetOverflow)?;
            self.vtable.data.push_aligned(d)?;
        }
        let entity_col_offset = self.vtable.data.extend_from_iter_aligned(entity_ids)? as u64;
        let data_len = len * arr_len * prim_type.size();

        let padding = prim_type.padding(self.data_len);
        let total_len = data_len.checked_add(padding).ok_or(Error::OffsetOverflow)?;
        let data_col_offset = (self.data_len + padding) as u64;
        self.data_len += total_len;

        let entry = ColumnEntry {
            len: len as u64,
            component_id,
            shape_entry: ShapeEntry {
                prim_type,
                rank,
                shape_offset,
            },
            entity_ids_entry: BufEntry {
                offset: entity_col_offset,
                phantom_data: PhantomData,
            },
            data_col_offset,
            timestamp_offset,
        };
        self.vtable.entries.push(Entry::Column(entry))?;
        Ok(self)
    }

    pub fn entity(
        &mut self,
        entity_id: EntityId,
        components: &[(ComponentId, PrimType, &[u64])],
    ) -> Result<&mut Self, Error> {
        self.entity_with_timestamp(entity_id, components, None)
    }

    pub fn entity_with_timestamp(
        &mut self,
        entity_id: EntityId,
        components: &[(ComponentId, PrimType, &[u64])],
        timestamp_offset: Option<u64>,
    ) -> Result<&mut Self, Error> {
        let len = components.len() as u64;
        let component_ids = components.iter().map(|(id, _, _)| *id);
        let component_ids_offset = self.vtable.data.extend_from_iter_aligned(component_ids)? as u64;

        self.vtable.data.pad_for_alignment::<ShapeEntry>()?;
        let shapes_offset = self.vtable.data.len();
        let shapes_len = mem::size_of::<ShapeEntry>() * components.len();
        for _ in 0..shapes_len {
            self.vtable.data.push(0)?;
        }
        for (i, (_, prim_type, shape)) in components.iter().enumerate() {
            let shape_offset = self.vtable.data.extend_aligned(shape)? as u64;
            let entry = ShapeEntry {
                prim_type: *prim_type,
                rank: shape.len() as u64,
                shape_offset,
            };
            let size = size_of::<ShapeEntry>();
            let offset = shapes_offset + i * size;
            self.vtable.data.as_mut_slice()[offset..offset + size]
                .copy_from_slice(entry.as_bytes());
        }

        let data_col_offset = self.data_len as u64;
        self.data_len += components
            .iter()
            .try_fold(0usize, |acc, (_, prim_type, shape)| {
                let arr_len: usize = shape.iter().try_fold(1usize, |xs, &x| {
                    (x as usize).checked_mul(xs).ok_or(Error::OffsetOverflow)
                })?;
                acc.checked_add(arr_len * prim_type.size())
                    .ok_or(Error::OffsetOverflow)
            })?;

        let entry = RowEntry {
            len,
            entity_id,
            component_ids_entry: BufEntry {
                offset: component_ids_offset,
                phantom_data: PhantomData,
            },
            shapes_entry: BufEntry {
                offset: shapes_offset as u64,
                phantom_data: PhantomData,
            },
            data_col_offset,
            timestamp_offset,
        };
        self.vtable.entries.push(Entry::Entity(entry))?;
        Ok(self)
    }
}

pub fn arr_len(shape: &[usize]) -> Result<usize, Error> {
    shape.iter().try_fold(1usize, |x, &xs| {
        x.checked_mul(xs).ok_or(Error::OffsetOverflow)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nox::{Array, ArrayBuf, Dyn, array};

    use super::*;

    #[derive(Default)]
    struct TestSink {
        f32_entities: HashMap<(ComponentId, EntityId), Array<f32, Dyn>>,
        f64_entities: HashMap<(ComponentId, EntityId), Array<f64, Dyn>>,
    }

    impl Decomponentize for TestSink {
        fn apply_value(
            &mut self,
            component_id: ComponentId,
            entity_id: EntityId,
            value: ComponentView<'_>,
            _timestamp: Option<Timestamp>,
        ) {
            match value {
                ComponentView::F32(view) => {
                    self.f32_entities
                        .insert((component_id, entity_id), view.to_dyn_owned());
                }

                ComponentView::F64(view) => {
                    self.f64_entities
                        .insert((component_id, entity_id), view.to_dyn_owned());
                }
                _ => todo!(),
            }
        }
    }

    #[test]
    fn test_parse_col_entry() -> Result<(), Error> {
        let mut vtable = VTableBuilder::default();
        vtable.column(
            ComponentId::new("foo"),
            PrimType::F32,
            [2, 2],
            [EntityId(1), EntityId(2)].into_iter(),
        )?;
        let vtable: VTable<Vec<Entry>, Vec<u8>> = vtable.build();

        let arr = nox::array![[[1f32, 0.0], [5.0, 5.0]], [[4.0, 4.0], [1.0, 1.0]]];
        let buf: &[f32] = arr.buf.as_buf();
        let table: &[u8] = buf.as_bytes();
        let mut sink = TestSink::default();
        vtable.parse_table(table, &mut sink)?;
        assert_eq!(
            *sink
                .f32_entities
                .get(&(ComponentId::new("foo"), EntityId(1)))
                .unwrap(),
            array![[1.0f32, 0.0], [5.0, 5.0]].to_dyn()
        );

        assert_eq!(
            *sink
                .f32_entities
                .get(&(ComponentId::new("foo"), EntityId(2)))
                .unwrap(),
            array![[4.0f32, 4.0], [1.0, 1.0]].to_dyn()
        );
        Ok(())
    }

    #[test]
    fn test_parse_row_entry() -> Result<(), Error> {
        let mut vtable = VTableBuilder::default();
        vtable.entity(
            EntityId(0),
            &[
                (ComponentId::new("foo"), PrimType::F32, &[2, 2]),
                (ComponentId::new("bar"), PrimType::F64, &[3]),
            ],
        )?;
        let vtable: VTable<Vec<Entry>, Vec<u8>> = vtable.build();

        let mut table = vec![];
        let arr = nox::array![[1f32, 0.0], [5.0, 5.0]];
        let buf: &[f32] = arr.buf.as_buf();
        table.extend_from_slice(buf.as_bytes());

        let arr = nox::array![1.0, 2.0, 3.0f64];
        let buf: &[f64] = arr.buf.as_buf();
        table.extend_from_slice(buf.as_bytes());
        let mut sink = TestSink::default();
        vtable.parse_table(&table, &mut sink)?;
        assert_eq!(
            *sink
                .f32_entities
                .get(&(ComponentId::new("foo"), EntityId(0)))
                .unwrap(),
            array![[1.0f32, 0.0], [5.0, 5.0]].to_dyn()
        );

        assert_eq!(
            *sink
                .f64_entities
                .get(&(ComponentId::new("bar"), EntityId(0)))
                .unwrap(),
            array![1., 2., 3.0f64].to_dyn()
        );
        Ok(())
    }
}

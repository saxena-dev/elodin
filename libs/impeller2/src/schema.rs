use serde::{Deserialize, Serialize};
use zerocopy::{FromBytes, IntoBytes};

use crate::{
    buf::Buf,
    error::Error,
    types::{ComponentId, PrimType},
};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Schema<S: Buf<u64>> {
    component_id: ComponentId,
    prim_type: PrimType,
    #[serde(bound(deserialize = ""))]
    shape: S,
}

impl<D: Buf<u64>> Schema<D> {
    pub fn new<T, I>(
        component_id: ComponentId,
        prim_type: PrimType,
        shape: I,
    ) -> Result<Self, Error>
    where
        T: DimElem,
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        let mut data = D::default();
        let shape = shape.into_iter();
        for dim in shape {
            data.push(dim.into_u64())?;
        }
        Ok(Self {
            component_id,
            shape: data,
            prim_type,
        })
    }

    pub fn component_id(&self) -> ComponentId {
        self.component_id
    }

    pub fn prim_type(&self) -> PrimType {
        self.prim_type
    }

    pub fn shape(&self) -> &[usize] {
        let bytes = self.shape.as_slice().as_bytes();
        <[usize]>::ref_from_bytes(bytes).unwrap()
    }
}

pub trait DimElem {
    fn into_u64(self) -> u64;
}

impl DimElem for u64 {
    fn into_u64(self) -> u64 {
        self
    }
}

impl DimElem for usize {
    fn into_u64(self) -> u64 {
        self as u64
    }
}

impl<T: DimElem + Copy> DimElem for &'_ T {
    fn into_u64(self) -> u64 {
        (*self).into_u64()
    }
}

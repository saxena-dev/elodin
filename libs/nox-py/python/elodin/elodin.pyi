from __future__ import annotations
from collections.abc import Sequence
import jax
from typing import Any, Optional, Union, Tuple, ClassVar, List, Protocol, Annotated
import polars as pl
from elodin import Archetype

class PrimitiveType:
    F64: PrimitiveType
    U64: PrimitiveType

class Integrator:
    Rk4: Integrator
    SemiImplicit: Integrator

class ComponentType:
    def __init__(self, ty: PrimitiveType, shape: Tuple[int, ...]): ...
    ty: PrimitiveType
    shape: jax.typing.ArrayLike
    U64: ClassVar[ComponentType]
    F64: ClassVar[ComponentType]
    F32: ClassVar[ComponentType]
    Edge: ClassVar[ComponentType]
    Quaternion: ClassVar[ComponentType]
    SpatialPosF64: ClassVar[ComponentType]
    SpatialMotionF64: ClassVar[ComponentType]

class PipelineBuilder:
    def init_var(self, name: str, ty: ComponentType): ...
    def var_arrays(self) -> list[jax.typing.ArrayLike]: ...

class Asset(Protocol):
    def asset_name(self) -> str: ...
    def bytes(self) -> bytes: ...

class WorldBuilder:
    def spawn(
        self,
        archetypes: Asset | Archetype | list[Archetype],
        name: Optional[str] = None,
    ) -> EntityId: ...
    def insert(
        self, id: EntityId, archetypes: Asset | Archetype | Sequence[Archetype]
    ): ...
    def insert_asset(self, asset: Asset) -> Handle: ...
    def run(
        self,
        system: Any,
        time_step: Optional[float] = None,
        client: Optional[Client] = None,
    ): ...
    def serve(
        self,
        system: Any,
        daemon: bool = False,
        time_step: Optional[float] = None,
        client: Optional[Client] = None,
        addr: Optional[str] = None,
    ): ...
    def build(
        self,
        system: Any,
        time_step: Optional[float] = None,
        client: Optional[Client] = None,
    ) -> Exec: ...

class EntityId:
    def __init__(self, id: int): ...

class Client:
    @staticmethod
    def cpu() -> Client: ...

class SpatialTransform:
    __metadata__: ClassVar[Tuple[Component,]]
    def __init__(
        self,
        arr: Optional[jax.typing.ArrayLike] = None,
        angular: Optional[Quaternion] = None,
        linear: Optional[jax.typing.ArrayLike] = None,
    ): ...
    @staticmethod
    def from_linear(linear: jax.typing.ArrayLike) -> SpatialTransform:
        """
        DEPRECATED: Use `SpatialTransform(linear=...)` instead.
        """
    @staticmethod
    def from_angular(
        quaternion: jax.typing.ArrayLike | Quaternion,
    ) -> SpatialTransform:
        """
        DEPRECATED: Use `SpatialTransform(angular=...)` instead.
        """
    @staticmethod
    def from_axis_angle(
        axis: jax.typing.ArrayLike, angle: jax.typing.ArrayLike
    ) -> SpatialTransform: ...
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...
    @staticmethod
    def from_array(arr: jax.typing.ArrayLike) -> SpatialTransform: ...
    @staticmethod
    def zero() -> SpatialTransform:
        """
        DEPRECATED: Use `SpatialTransform()` instead.
        """
    def linear(self) -> jax.Array: ...
    def angular(self) -> Quaternion: ...
    def asarray(self) -> jax.typing.ArrayLike: ...
    def __add__(self, other: SpatialTransform) -> SpatialTransform: ...

class SpatialForce:
    __metadata__: ClassVar[Tuple[Component,]]
    def __init__(self, arr: jax.typing.ArrayLike): ...
    @staticmethod
    def from_array(arr: jax.typing.ArrayLike) -> SpatialForce: ...
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...
    def asarray(self) -> jax.typing.ArrayLike: ...
    @staticmethod
    def zero() -> SpatialForce: ...
    @staticmethod
    def from_linear(linear: jax.typing.ArrayLike) -> SpatialForce: ...
    @staticmethod
    def from_torque(linear: jax.typing.ArrayLike) -> SpatialForce: ...
    def force(self) -> jax.typing.ArrayLike: ...
    def torque(self) -> jax.typing.ArrayLike: ...
    def __add__(self, other: SpatialForce) -> SpatialForce: ...

class SpatialMotion:
    __metadata__: ClassVar[Tuple[Component,]]
    def __init__(self, angular: jax.typing.ArrayLike, linear: jax.typing.ArrayLike): ...
    @staticmethod
    def from_array(arr: jax.typing.ArrayLike) -> SpatialMotion: ...
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...
    def asarray(self) -> jax.typing.ArrayLike: ...
    @staticmethod
    def zero() -> SpatialMotion: ...
    @staticmethod
    def from_linear(linear: jax.typing.ArrayLike) -> SpatialMotion: ...
    @staticmethod
    def from_angular(angular: jax.typing.ArrayLike) -> SpatialMotion: ...
    def linear(self) -> jax.Array: ...
    def angular(self) -> jax.Array: ...
    def __add__(self, other: SpatialMotion) -> SpatialMotion: ...

class SpatialInertia:
    __metadata__: ClassVar[Tuple[Component,]]
    def __init__(
        self, mass: jax.typing.ArrayLike, inertia: Optional[jax.typing.ArrayLike] = None
    ): ...
    @staticmethod
    def from_array(arr: jax.typing.ArrayLike) -> SpatialInertia: ...
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...
    def asarray(self) -> jax.typing.ArrayLike: ...
    def mass(self) -> jax.typing.ArrayLike: ...
    def inertia_diag(self) -> jax.typing.ArrayLike: ...

class Quaternion:
    __metadata__: ClassVar[Tuple[Component,]]
    def __init__(self, arr: jax.typing.ArrayLike): ...
    @staticmethod
    def from_array(arr: jax.typing.ArrayLike) -> Quaternion: ...
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...
    def asarray(self) -> jax.typing.ArrayLike: ...
    @staticmethod
    def identity() -> Quaternion: ...
    @staticmethod
    def from_axis_angle(
        axis: jax.typing.ArrayLike, angle: jax.typing.ArrayLike
    ) -> Quaternion: ...
    def vector(self) -> jax.Array: ...
    def normalize(self) -> Quaternion: ...
    def __mul__(self, other: Quaternion) -> Quaternion: ...
    def __add__(self, other: Quaternion) -> Quaternion: ...
    def __matmul__(
        self, vector: jax.Array | SpatialTransform | SpatialMotion | SpatialForce
    ) -> jax.Array: ...
    def inverse(self) -> Quaternion: ...

class RustSystem:
    def call(self, builder: PipelineBuilder): ...
    def init(self, builder: PipelineBuilder): ...
    def pipe(self, other: Any) -> RustSystem: ...
    def __or__(self, other: Any) -> RustSystem: ...

class Mesh:
    @staticmethod
    def cuboid(x: float, y: float, z: float) -> Mesh: ...
    @staticmethod
    def sphere(radius: float) -> Mesh: ...
    def asset_name(self) -> str: ...
    def bytes(self) -> bytes: ...

class Material:
    @staticmethod
    def color(r: float, g: float, b: float) -> Material: ...
    def asset_name(self) -> str: ...
    def bytes(self) -> bytes: ...

class Texture: ...

class Handle:
    __metadata__: ClassVar[Tuple[Component,]]
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...

class Pbr:
    def __init__(self, mesh: Mesh, material: Material): ...
    @staticmethod
    def from_url(url: str) -> Pbr: ...
    @staticmethod
    def from_path(path: str) -> Pbr: ...
    def asset_name(self) -> str: ...
    def bytes(self) -> bytes: ...

class Metadata:
    ty: ComponentType
    @staticmethod
    def of(component: Annotated[Any, Component]) -> Metadata: ...

class QueryInner:
    def join_query(self, other: QueryInner) -> QueryInner: ...
    def arrays(self) -> list[jax.Array]: ...
    def map(self, ty: jax.Array, f: Metadata) -> Any: ...
    @staticmethod
    def from_builder(builder: PipelineBuilder, names: list[str]) -> QueryInner: ...
    def insert_into_builder(self, builder: PipelineBuilder) -> None: ...

class GraphQueryInner:
    def arrays(
        self, from_query: QueryInner, to_query: QueryInner
    ) -> dict[int, Tuple[list[jax.Array], list[jax.Array]]]: ...
    @staticmethod
    def from_builder(
        builder: PipelineBuilder, edge_name: str, reverse: bool
    ) -> GraphQueryInner: ...
    @staticmethod
    def from_builder_total_edge(builder: PipelineBuilder) -> GraphQueryInner: ...
    def insert_into_builder(self, builder: PipelineBuilder) -> None: ...
    def map(
        self,
        from_query: QueryInner,
        to_query: QueryInner,
        ty: jax.typing.ArrayLike,
        f: Metadata,
    ) -> QueryInner: ...

class Edge:
    __metadata__: ClassVar[Tuple[Component,]]
    def __init__(self, left: EntityId, right: EntityId): ...
    def flatten(self) -> Any: ...
    @staticmethod
    def unflatten(aux: Any, jax: Any) -> Any: ...

class Component:
    asset: bool
    def __init__(
        self,
        name: str,
        ty: Optional[ComponentType] = None,
        asset: bool = False,
        metadata: dict[str, str | bool | int] = {},
    ): ...
    # DEPRECATED: Use Component.name instead
    @staticmethod
    def id(component: Any) -> str: ...
    @staticmethod
    def name(component: Any) -> str: ...
    @staticmethod
    def index(component: Any) -> ShapeIndexer: ...

class ShapeIndexer:
    def __getitem__(self, index: Any) -> ShapeIndexer: ...

class Conduit:
    @staticmethod
    def tcp(addr: str) -> Conduit: ...

class Exec:
    def run(self, ticks: int = 1, show_progress: bool = True): ...
    def profile(self) -> dict[str, float]: ...
    def write_to_dir(self, path: str): ...
    def history(self) -> pl.DataFrame: ...
    def column_array(self, name: str) -> pl.Series: ...

class Color:
    def __init__(self, r: float, g: float, b: float): ...

class Gizmo:
    @staticmethod
    def vector(name: str, offset: int, color: Color) -> jax.Array: ...

class Panel:
    @staticmethod
    def vsplit(panels: list[Panel], active: bool = False) -> Panel: ...
    @staticmethod
    def hsplit(panels: list[Panel], active: bool = False) -> Panel: ...
    @staticmethod
    def viewport(
        track_entity: Optional[EntityId] = None,
        track_rotation: bool = True,
        fov: Optional[float] = None,
        active: bool = False,
        pos: Union[List[float], jax.Array, None] = None,
        looking_at: Union[List[float], jax.Array, None] = None,
        show_grid: bool = False,
        hdr: bool = False,
    ) -> Panel: ...
    @staticmethod
    def graph(entities: list[GraphEntity]) -> Panel: ...
    def asset_name(self) -> str: ...
    def bytes(self) -> bytes: ...

class GraphEntity:
    def __init__(
        self, entity_id: EntityId, components: list[GraphComponent | ShapeIndexer]
    ): ...

class GraphComponent:
    def __init__(self, component_name: str, indexes: list[int]): ...

class Glb:
    def __init__(self, path: str): ...
    def bytes(self) -> bytes: ...

def six_dof(
    time_step: float, sys: Any = None, integrator: Integrator = Integrator.Rk4
) -> RustSystem: ...
def advance_time(time_step: float) -> RustSystem: ...
def read_batch_results(path: str) -> Tuple[list[pl.DataFrame], list[int]]: ...
def skew(arr: jax.Array) -> jax.Array: ...

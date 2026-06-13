use super::{
    ContainerData, ContainerKey, Elements, InsertError, Key, TextData, TextKey,
    layout::{ChildMeasure, Context, LayoutBehaviour, Measure, Placement, ResolveResult},
};


pub(crate) trait ShapeStore {
    type DataKey: Copy;
    type Data;

    fn insert(elements: &mut Elements, data: Self::Data) -> Self::DataKey;
    fn get(elements: &Elements, key: Self::DataKey) -> &Self::Data;
    fn get_mut(elements: &mut Elements, key: Self::DataKey) -> &mut Self::Data;
}

pub(crate) struct TextStore;

impl ShapeStore for TextStore {
    type DataKey = TextKey;
    type Data = TextData;

    fn insert(elements: &mut Elements, data: Self::Data) -> Self::DataKey {
        elements.texts.insert(data)
    }

    fn get(elements: &Elements, key: Self::DataKey) -> &Self::Data {
        &elements.texts[key]
    }

    fn get_mut(elements: &mut Elements, key: Self::DataKey) -> &mut Self::Data {
        &mut elements.texts[key]
    }
}

pub(crate) struct ContainerStore;

impl ShapeStore for ContainerStore {
    type DataKey = ContainerKey;
    type Data = ContainerData;

    fn insert(elements: &mut Elements, data: Self::Data) -> Self::DataKey {
        elements.containers.insert(data)
    }

    fn get(elements: &Elements, key: Self::DataKey) -> &Self::Data {
        &elements.containers[key]
    }

    fn get_mut(elements: &mut Elements, key: Self::DataKey) -> &mut Self::Data {
        &mut elements.containers[key]
    }
}

pub trait ShapeMeta {
    type ElementSpec;

    const CAN_BE_ROOT: bool;
}

pub(crate) trait ElementInsert: ShapeMeta {
    type Store: ShapeStore;

    fn data_from_spec(spec: Self::ElementSpec) -> <Self::Store as ShapeStore>::Data;

    //fn update(data: &mut <Self::Store as ShapeStore>::Data, init: Self::ElementSpec) -> bool;

    fn valid_as_parent(
        data_key: <Self::Store as ShapeStore>::DataKey,
        data: &<Self::Store as ShapeStore>::Data,
    ) -> Result<ContainerKey, InsertError>;

    fn source_children(_data: &<Self::Store as ShapeStore>::Data) -> &[Key] {
        &[]
    }
}

macro_rules! define_shapes {
    ($($variant:ident => $shape:ty),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum Shape {
            $($variant(<<$shape as ElementInsert>::Store as ShapeStore>::DataKey),)+
        }

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(crate) enum ShapeRef<'a> {
            $($variant(<<$shape as ElementInsert>::Store as ShapeStore>::DataKey, &'a <<$shape as ElementInsert>::Store as ShapeStore>::Data),)+
        }

        pub enum ElementSpec {
            $($variant(<$shape as ShapeMeta>::ElementSpec),)+
        }

        impl ElementSpec {
            pub(crate) fn insert(self, elements: &mut Elements) -> Shape {
                match self {
                    $(Self::$variant(init) => {
                        let data_key = <<$shape as ElementInsert>::Store as ShapeStore>::insert(
                            elements,
                            <$shape as ElementInsert>::data_from_spec(init),
                        );
                        Shape::$variant(data_key)
                    },)+
                }
            }

            pub fn can_be_root(&self) -> bool {
                match self {
                    $(Self::$variant(_) => <$shape as ShapeMeta>::CAN_BE_ROOT,)+
                }
            }
        }

        impl Shape {
            pub(crate) fn bind<'a>(self, elements: &'a Elements) -> ShapeRef<'a> {
                match self {
                    $(Self::$variant(data_key) => {
                        ShapeRef::$variant(
                            data_key,
                            <<$shape as ElementInsert>::Store as ShapeStore>::get(elements, data_key),
                        )
                    },)+
                }
            }

            /*
            pub(crate) fn update(
                self,
                elements: &mut Elements,
                init: ElementSpec,
            ) -> Result<bool, UpdateError> {
                match (self, init) {
                    $((Self::$variant(data_key), ElementSpec::$variant(init)) => {
                        Ok(<$shape as ElementInsert>::update(
                            <<$shape as ElementInsert>::Store as ShapeStore>::get_mut(
                                elements,
                                data_key,
                            ),
                            init,
                        ))
                    },)+
                    _ => Err(UpdateError::WrongShape),
                }
            }
            */


        }

        impl<'a> ShapeRef<'a> {
            pub(crate) fn source_children(self) -> &'a [Key] {
                match self {
                    $(Self::$variant(_, data) => {
                        <$shape as ElementInsert>::source_children(data)
                    },)+
                }
            }

            pub(crate) fn valid_as_parent( self) -> Result<ContainerKey, InsertError> {
                match self {
                    $(Self::$variant(data_key, data) => {
                        <$shape as ElementInsert>::valid_as_parent(data_key, data)
                    },)+
                }
            }

            pub(crate) fn context(self, parent: &Context) -> Context {
                match self {
                    $(Self::$variant(_, data) => {
                        <$shape as LayoutBehaviour<&<<$shape as ElementInsert>::Store as ShapeStore>::Data>>::context(
                            data,
                            parent,
                        )
                    },)+
                }
            }

            pub(crate) fn measure(
                self,
                context: &Context,
                children: &[ChildMeasure<'_>],
            ) -> Measure {
                match self {
                    $(Self::$variant(_,_) => {
                        <$shape as LayoutBehaviour<&<<$shape as ElementInsert>::Store as ShapeStore>::Data>>::measure(
                            context,
                            children,
                        )
                    },)+
                }
            }

            pub(crate) fn resolve(
                self,
                context: &Context,
                measure: &Measure,
                children: &[ChildMeasure<'_>],
                parent: &Placement,
            ) -> ResolveResult {
                match self {
                    $(Self::$variant(_,_) => {
                        <$shape as LayoutBehaviour<&<<$shape as ElementInsert>::Store as ShapeStore>::Data>>::resolve(
                            context,
                            measure,
                            children,
                            parent,
                        )
                    },)+
                }
            }
        }
    };
}

define_shapes! {
    Text => text::Text,
    El => el::El,
//    Row => row::Row,
}

pub(crate) mod text;
pub(crate) mod el;
//mod row;

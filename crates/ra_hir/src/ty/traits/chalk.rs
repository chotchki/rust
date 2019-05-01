//! Conversion code from/to Chalk.
use std::sync::Arc;

use chalk_ir::{TypeId, ImplId, TypeKindId, ProjectionTy, Parameter, Identifier, cast::Cast, PlaceholderIndex, UniverseIndex, TypeName};
use chalk_rust_ir::{AssociatedTyDatum, TraitDatum, StructDatum, ImplDatum};

use ra_db::salsa::{InternId, InternKey};

use crate::{
    Trait, HasGenericParams, ImplBlock,
    db::HirDatabase,
    ty::{TraitRef, Ty, ApplicationTy, TypeCtor, Substs},
};
use super::ChalkContext;

pub(super) trait ToChalk {
    type Chalk;
    fn to_chalk(self, db: &impl HirDatabase) -> Self::Chalk;
    fn from_chalk(db: &impl HirDatabase, chalk: Self::Chalk) -> Self;
}

pub(super) fn from_chalk<T, ChalkT>(db: &impl HirDatabase, chalk: ChalkT) -> T
where
    T: ToChalk<Chalk = ChalkT>,
{
    T::from_chalk(db, chalk)
}

impl ToChalk for Ty {
    type Chalk = chalk_ir::Ty;
    fn to_chalk(self, db: &impl HirDatabase) -> chalk_ir::Ty {
        match self {
            Ty::Apply(apply_ty) => chalk_ir::Ty::Apply(apply_ty.to_chalk(db)),
            Ty::Param { idx, .. } => {
                PlaceholderIndex { ui: UniverseIndex::ROOT, idx: idx as usize }.to_ty()
            }
            Ty::Bound(idx) => chalk_ir::Ty::BoundVar(idx as usize),
            Ty::Infer(_infer_ty) => panic!("uncanonicalized infer ty"),
            Ty::Unknown => unimplemented!(), // TODO turn into placeholder?
        }
    }
    fn from_chalk(db: &impl HirDatabase, chalk: chalk_ir::Ty) -> Self {
        match chalk {
            chalk_ir::Ty::Apply(apply_ty) => {
                match apply_ty.name {
                    // FIXME handle TypeKindId::Trait/Type here
                    TypeName::TypeKindId(_) => Ty::Apply(from_chalk(db, apply_ty)),
                    TypeName::AssociatedType(_) => unimplemented!(),
                    TypeName::Placeholder(idx) => {
                        assert_eq!(idx.ui, UniverseIndex::ROOT);
                        Ty::Param { idx: idx.idx as u32, name: crate::Name::missing() }
                    }
                }
            }
            chalk_ir::Ty::Projection(_) => unimplemented!(),
            chalk_ir::Ty::UnselectedProjection(_) => unimplemented!(),
            chalk_ir::Ty::ForAll(_) => unimplemented!(),
            chalk_ir::Ty::BoundVar(idx) => Ty::Bound(idx as u32),
            chalk_ir::Ty::InferenceVar(_iv) => panic!("unexpected chalk infer ty"),
        }
    }
}

impl ToChalk for ApplicationTy {
    type Chalk = chalk_ir::ApplicationTy;

    fn to_chalk(self: ApplicationTy, db: &impl HirDatabase) -> chalk_ir::ApplicationTy {
        let struct_id = self.ctor.to_chalk(db);
        let name = TypeName::TypeKindId(struct_id.into());
        let parameters = self.parameters.to_chalk(db);
        chalk_ir::ApplicationTy { name, parameters }
    }

    fn from_chalk(db: &impl HirDatabase, apply_ty: chalk_ir::ApplicationTy) -> ApplicationTy {
        let ctor = match apply_ty.name {
            TypeName::TypeKindId(TypeKindId::StructId(struct_id)) => from_chalk(db, struct_id),
            TypeName::TypeKindId(_) => unimplemented!(),
            TypeName::Placeholder(_) => unimplemented!(),
            TypeName::AssociatedType(_) => unimplemented!(),
        };
        let parameters = from_chalk(db, apply_ty.parameters);
        ApplicationTy { ctor, parameters }
    }
}

impl ToChalk for Substs {
    type Chalk = Vec<chalk_ir::Parameter>;

    fn to_chalk(self, db: &impl HirDatabase) -> Vec<Parameter> {
        self.iter().map(|ty| ty.clone().to_chalk(db).cast()).collect()
    }

    fn from_chalk(db: &impl HirDatabase, parameters: Vec<chalk_ir::Parameter>) -> Substs {
        parameters
            .into_iter()
            .map(|p| match p {
                chalk_ir::Parameter(chalk_ir::ParameterKind::Ty(ty)) => from_chalk(db, ty),
                chalk_ir::Parameter(chalk_ir::ParameterKind::Lifetime(_)) => unimplemented!(),
            })
            .collect::<Vec<_>>()
            .into()
    }
}

impl ToChalk for TraitRef {
    type Chalk = chalk_ir::TraitRef;

    fn to_chalk(self: TraitRef, db: &impl HirDatabase) -> chalk_ir::TraitRef {
        let trait_id = self.trait_.to_chalk(db);
        let parameters = self.substs.to_chalk(db);
        chalk_ir::TraitRef { trait_id, parameters }
    }

    fn from_chalk(db: &impl HirDatabase, trait_ref: chalk_ir::TraitRef) -> Self {
        let trait_ = from_chalk(db, trait_ref.trait_id);
        let substs = from_chalk(db, trait_ref.parameters);
        TraitRef { trait_, substs }
    }
}

impl ToChalk for Trait {
    type Chalk = chalk_ir::TraitId;

    fn to_chalk(self, _db: &impl HirDatabase) -> chalk_ir::TraitId {
        self.id.into()
    }

    fn from_chalk(_db: &impl HirDatabase, trait_id: chalk_ir::TraitId) -> Trait {
        Trait { id: trait_id.into() }
    }
}

impl ToChalk for TypeCtor {
    type Chalk = chalk_ir::StructId;

    fn to_chalk(self, db: &impl HirDatabase) -> chalk_ir::StructId {
        db.intern_type_ctor(self).into()
    }

    fn from_chalk(db: &impl HirDatabase, struct_id: chalk_ir::StructId) -> TypeCtor {
        db.lookup_intern_type_ctor(struct_id.into())
    }
}

impl ToChalk for ImplBlock {
    type Chalk = chalk_ir::ImplId;

    fn to_chalk(self, db: &impl HirDatabase) -> chalk_ir::ImplId {
        db.intern_impl_block(self).into()
    }

    fn from_chalk(db: &impl HirDatabase, impl_id: chalk_ir::ImplId) -> ImplBlock {
        db.lookup_intern_impl_block(impl_id.into())
    }
}

fn make_binders<T>(value: T, num_vars: usize) -> chalk_ir::Binders<T> {
    chalk_ir::Binders {
        value,
        binders: std::iter::repeat(chalk_ir::ParameterKind::Ty(())).take(num_vars).collect(),
    }
}

impl<'a, DB> chalk_solve::RustIrDatabase for ChalkContext<'a, DB>
where
    DB: HirDatabase,
{
    fn associated_ty_data(&self, _ty: TypeId) -> Arc<AssociatedTyDatum> {
        unimplemented!()
    }
    fn trait_datum(&self, trait_id: chalk_ir::TraitId) -> Arc<TraitDatum> {
        eprintln!("trait_datum {:?}", trait_id);
        let trait_: Trait = from_chalk(self.db, trait_id);
        let generic_params = trait_.generic_params(self.db);
        let bound_vars = Substs::bound_vars(&generic_params);
        let trait_ref = trait_.trait_ref(self.db).subst(&bound_vars).to_chalk(self.db);
        let flags = chalk_rust_ir::TraitFlags {
            // FIXME set these flags correctly
            auto: false,
            marker: false,
            upstream: trait_.module(self.db).krate(self.db) != Some(self.krate),
            fundamental: false,
        };
        let where_clauses = Vec::new(); // FIXME add where clauses
        let trait_datum_bound = chalk_rust_ir::TraitDatumBound { trait_ref, where_clauses, flags };
        let trait_datum = TraitDatum { binders: make_binders(trait_datum_bound, bound_vars.len()) };
        Arc::new(trait_datum)
    }
    fn struct_datum(&self, struct_id: chalk_ir::StructId) -> Arc<StructDatum> {
        eprintln!("struct_datum {:?}", struct_id);
        let type_ctor = from_chalk(self.db, struct_id);
        // TODO might be nicer if we can create a fake GenericParams for the TypeCtor
        let (num_params, upstream) = match type_ctor {
            TypeCtor::Bool
            | TypeCtor::Char
            | TypeCtor::Int(_)
            | TypeCtor::Float(_)
            | TypeCtor::Never
            | TypeCtor::Str => (0, true),
            TypeCtor::Slice | TypeCtor::Array | TypeCtor::RawPtr(_) | TypeCtor::Ref(_) => (1, true),
            TypeCtor::FnPtr | TypeCtor::Tuple => unimplemented!(), // FIXME tuples and FnPtr are currently variadic... we need to make the parameter number explicit
            TypeCtor::FnDef(_) => unimplemented!(),
            TypeCtor::Adt(adt) => {
                let generic_params = adt.generic_params(self.db);
                (
                    generic_params.count_params_including_parent(),
                    adt.krate(self.db) != Some(self.krate),
                )
            }
        };
        let flags = chalk_rust_ir::StructFlags {
            upstream,
            // FIXME set fundamental flag correctly
            fundamental: false,
        };
        let where_clauses = Vec::new(); // FIXME add where clauses
        let ty = ApplicationTy {
            ctor: type_ctor,
            parameters: (0..num_params).map(|i| Ty::Bound(i as u32)).collect::<Vec<_>>().into(),
        };
        let struct_datum_bound = chalk_rust_ir::StructDatumBound {
            self_ty: ty.to_chalk(self.db),
            fields: Vec::new(), // FIXME add fields (only relevant for auto traits)
            where_clauses,
            flags,
        };
        let struct_datum = StructDatum { binders: make_binders(struct_datum_bound, num_params) };
        Arc::new(struct_datum)
    }
    fn impl_datum(&self, impl_id: ImplId) -> Arc<ImplDatum> {
        eprintln!("impl_datum {:?}", impl_id);
        let impl_block: ImplBlock = from_chalk(self.db, impl_id);
        let generic_params = impl_block.generic_params(self.db);
        let bound_vars = Substs::bound_vars(&generic_params);
        let trait_ref = impl_block
            .target_trait_ref(self.db)
            .expect("FIXME handle unresolved impl block trait ref")
            .subst(&bound_vars);
        let impl_type = if impl_block.module().krate(self.db) == Some(self.krate) {
            chalk_rust_ir::ImplType::Local
        } else {
            chalk_rust_ir::ImplType::External
        };
        let impl_datum_bound = chalk_rust_ir::ImplDatumBound {
            // FIXME handle negative impls (impl !Sync for Foo)
            trait_ref: chalk_rust_ir::PolarizedTraitRef::Positive(trait_ref.to_chalk(self.db)),
            where_clauses: Vec::new(),        // FIXME add where clauses
            associated_ty_values: Vec::new(), // FIXME add associated type values
            impl_type,
        };
        let impl_datum = ImplDatum { binders: make_binders(impl_datum_bound, bound_vars.len()) };
        Arc::new(impl_datum)
    }
    fn impls_for_trait(&self, trait_id: chalk_ir::TraitId) -> Vec<ImplId> {
        eprintln!("impls_for_trait {:?}", trait_id);
        let trait_ = from_chalk(self.db, trait_id);
        self.db
            .impls_for_trait(self.krate, trait_)
            .iter()
            // FIXME temporary hack -- as long as we're not lowering where clauses
            // correctly, ignore impls with them completely so as to not treat
            // impl<T> Trait for T where T: ... as a blanket impl on all types
            .filter(|impl_block| impl_block.generic_params(self.db).where_predicates.is_empty())
            .map(|impl_block| impl_block.to_chalk(self.db))
            .collect()
    }
    fn impl_provided_for(
        &self,
        auto_trait_id: chalk_ir::TraitId,
        struct_id: chalk_ir::StructId,
    ) -> bool {
        eprintln!("impl_provided_for {:?}, {:?}", auto_trait_id, struct_id);
        false // FIXME
    }
    fn type_name(&self, _id: TypeKindId) -> Identifier {
        unimplemented!()
    }
    fn split_projection<'p>(
        &self,
        projection: &'p ProjectionTy,
    ) -> (Arc<AssociatedTyDatum>, &'p [Parameter], &'p [Parameter]) {
        eprintln!("split_projection {:?}", projection);
        unimplemented!()
    }
}

fn id_from_chalk<T: InternKey>(chalk_id: chalk_ir::RawId) -> T {
    T::from_intern_id(InternId::from(chalk_id.index))
}
fn id_to_chalk<T: InternKey>(salsa_id: T) -> chalk_ir::RawId {
    chalk_ir::RawId { index: salsa_id.as_intern_id().as_u32() }
}

impl From<chalk_ir::TraitId> for crate::ids::TraitId {
    fn from(trait_id: chalk_ir::TraitId) -> Self {
        id_from_chalk(trait_id.0)
    }
}

impl From<crate::ids::TraitId> for chalk_ir::TraitId {
    fn from(trait_id: crate::ids::TraitId) -> Self {
        chalk_ir::TraitId(id_to_chalk(trait_id))
    }
}

impl From<chalk_ir::StructId> for crate::ids::TypeCtorId {
    fn from(struct_id: chalk_ir::StructId) -> Self {
        id_from_chalk(struct_id.0)
    }
}

impl From<crate::ids::TypeCtorId> for chalk_ir::StructId {
    fn from(type_ctor_id: crate::ids::TypeCtorId) -> Self {
        chalk_ir::StructId(id_to_chalk(type_ctor_id))
    }
}

impl From<chalk_ir::ImplId> for crate::ids::GlobalImplId {
    fn from(impl_id: chalk_ir::ImplId) -> Self {
        id_from_chalk(impl_id.0)
    }
}

impl From<crate::ids::GlobalImplId> for chalk_ir::ImplId {
    fn from(impl_id: crate::ids::GlobalImplId) -> Self {
        chalk_ir::ImplId(id_to_chalk(impl_id))
    }
}

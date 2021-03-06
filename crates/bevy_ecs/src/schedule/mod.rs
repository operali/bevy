mod executor;
mod executor_parallel;
mod label;
mod stage;
//mod stageless;
mod state;
mod system_container;
mod system_descriptor;
mod system_set;

pub use executor::*;
pub use executor_parallel::*;
pub use label::*;
pub use stage::*;
pub use state::*;
pub use system_container::*;
pub use system_descriptor::*;
pub use system_set::*;

use crate::{
    ArchetypeComponent, BoxedSystem, IntoSystem, Resources, System, SystemId, TypeAccess, World,
};
use bevy_utils::HashMap;
use std::{any::TypeId, borrow::Cow};

#[derive(Default)]
pub struct Schedule {
    stages: HashMap<BoxedStageLabel, Box<dyn Stage>>,
    stage_order: Vec<BoxedStageLabel>,
    run_criteria: RunCriteria,
}

impl Schedule {
    pub fn with_stage<S: Stage>(mut self, label: impl StageLabel, stage: S) -> Self {
        self.add_stage(label, stage);
        self
    }

    pub fn with_stage_after<S: Stage>(
        mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> Self {
        self.add_stage_after(target, label, stage);
        self
    }

    pub fn with_stage_before<S: Stage>(
        mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> Self {
        self.add_stage_before(target, label, stage);
        self
    }

    pub fn with_run_criteria<S: System<In = (), Out = ShouldRun>>(mut self, system: S) -> Self {
        self.set_run_criteria(system);
        self
    }

    pub fn with_system_in_stage(
        mut self,
        stage_label: impl StageLabel,
        system: impl Into<SystemDescriptor>,
    ) -> Self {
        self.add_system_to_stage(stage_label, system);
        self
    }

    pub fn set_run_criteria<S: System<In = (), Out = ShouldRun>>(
        &mut self,
        system: S,
    ) -> &mut Self {
        self.run_criteria.set(Box::new(system.system()));
        self
    }

    pub fn add_stage<S: Stage>(&mut self, label: impl StageLabel, stage: S) -> &mut Self {
        let label: Box<dyn StageLabel> = Box::new(label);
        self.stage_order.push(label.clone());
        let prev = self.stages.insert(label.clone(), Box::new(stage));
        if prev.is_some() {
            panic!("Stage already exists: {:?}.", label);
        }
        self
    }

    pub fn add_stage_after<S: Stage>(
        &mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> &mut Self {
        let label: Box<dyn StageLabel> = Box::new(label);
        let target = &target as &dyn StageLabel;
        let target_index = self
            .stage_order
            .iter()
            .enumerate()
            .find(|(_i, stage_label)| &***stage_label == target)
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("Target stage does not exist: {:?}.", target));

        self.stage_order.insert(target_index + 1, label.clone());
        let prev = self.stages.insert(label.clone(), Box::new(stage));
        if prev.is_some() {
            panic!("Stage already exists: {:?}.", label);
        }
        self
    }

    pub fn add_stage_before<S: Stage>(
        &mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> &mut Self {
        let label: Box<dyn StageLabel> = Box::new(label);
        let target = &target as &dyn StageLabel;
        let target_index = self
            .stage_order
            .iter()
            .enumerate()
            .find(|(_i, stage_label)| &***stage_label == target)
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("Target stage does not exist: {:?}.", target));

        self.stage_order.insert(target_index, label.clone());
        let prev = self.stages.insert(label.clone(), Box::new(stage));
        if prev.is_some() {
            panic!("Stage already exists: {:?}.", label);
        }
        self
    }

    pub fn add_system_to_stage(
        &mut self,
        stage_label: impl StageLabel,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        let stage = self
            .get_stage_mut::<SystemStage>(&stage_label)
            .unwrap_or_else(move || {
                panic!(
                    "Stage '{:?}' does not exist or is not a SystemStage",
                    stage_label
                )
            });
        stage.add_system(system);
        self
    }

    pub fn add_system_set_to_stage(
        &mut self,
        stage_label: impl StageLabel,
        system_set: SystemSet,
    ) -> &mut Self {
        self.stage(stage_label, |stage: &mut SystemStage| {
            stage.add_system_set(system_set)
        })
    }

    pub fn stage<T: Stage, F: FnOnce(&mut T) -> &mut T>(
        &mut self,
        label: impl StageLabel,
        func: F,
    ) -> &mut Self {
        let stage = self.get_stage_mut::<T>(&label).unwrap_or_else(move || {
            panic!("stage '{:?}' does not exist or is the wrong type", label)
        });
        func(stage);
        self
    }

    pub fn get_stage<T: Stage>(&self, label: &dyn StageLabel) -> Option<&T> {
        self.stages
            .get(label)
            .and_then(|stage| stage.downcast_ref::<T>())
    }

    pub fn get_stage_mut<T: Stage>(&mut self, label: &dyn StageLabel) -> Option<&mut T> {
        self.stages
            .get_mut(label)
            .and_then(|stage| stage.downcast_mut::<T>())
    }

    pub fn run_once(&mut self, world: &mut World, resources: &mut Resources) {
        for label in self.stage_order.iter() {
            #[cfg(feature = "trace")]
            let stage_span =
                bevy_utils::tracing::info_span!("stage", name = &format!("{:?}", label) as &str);
            #[cfg(feature = "trace")]
            let _stage_guard = stage_span.enter();
            let stage = self.stages.get_mut(label).unwrap();
            stage.run(world, resources);
        }
    }
}

impl Stage for Schedule {
    fn run(&mut self, world: &mut World, resources: &mut Resources) {
        loop {
            match self.run_criteria.should_run(world, resources) {
                ShouldRun::No => return,
                ShouldRun::Yes => {
                    self.run_once(world, resources);
                    return;
                }
                ShouldRun::YesAndCheckAgain => {
                    self.run_once(world, resources);
                }
                ShouldRun::NoAndCheckAgain => {
                    panic!("`NoAndCheckAgain` would loop infinitely in this situation.")
                }
            }
        }
    }
}

pub fn clear_trackers_system(world: &mut World, resources: &mut Resources) {
    world.clear_trackers();
    resources.clear_trackers();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShouldRun {
    /// Yes, the system should run.
    Yes,
    /// No, the system should not run.
    No,
    /// Yes, the system should run, and afterwards the criteria should be checked again.
    YesAndCheckAgain,
    /// No, the system should not run right now, but the criteria should be checked again later.
    NoAndCheckAgain,
}

pub(crate) struct RunCriteria {
    criteria_system: Option<BoxedSystem<(), ShouldRun>>,
    initialized: bool,
}

impl Default for RunCriteria {
    fn default() -> Self {
        Self {
            criteria_system: None,
            initialized: false,
        }
    }
}

impl RunCriteria {
    pub fn set(&mut self, criteria_system: BoxedSystem<(), ShouldRun>) {
        self.criteria_system = Some(criteria_system);
        self.initialized = false;
    }

    pub fn should_run(&mut self, world: &mut World, resources: &mut Resources) -> ShouldRun {
        if let Some(ref mut run_criteria) = self.criteria_system {
            if !self.initialized {
                run_criteria.initialize(world, resources);
                self.initialized = true;
            }
            let should_run = run_criteria.run((), world, resources);
            run_criteria.apply_buffers(world, resources);
            // don't run when no result is returned or false is returned
            should_run.unwrap_or(ShouldRun::No)
        } else {
            ShouldRun::Yes
        }
    }
}

pub struct RunOnce {
    ran: bool,
    system_id: SystemId,
    archetype_component_access: TypeAccess<ArchetypeComponent>,
    component_access: TypeAccess<TypeId>,
    resource_access: TypeAccess<TypeId>,
}

impl Default for RunOnce {
    fn default() -> Self {
        Self {
            ran: false,
            system_id: SystemId::new(),
            archetype_component_access: Default::default(),
            component_access: Default::default(),
            resource_access: Default::default(),
        }
    }
}

impl System for RunOnce {
    type In = ();
    type Out = ShouldRun;

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(std::any::type_name::<RunOnce>())
    }

    fn id(&self) -> SystemId {
        self.system_id
    }

    fn update_access(&mut self, _world: &World) {}

    fn archetype_component_access(&self) -> &TypeAccess<ArchetypeComponent> {
        &self.archetype_component_access
    }

    fn component_access(&self) -> &TypeAccess<TypeId> {
        &self.component_access
    }

    fn resource_access(&self) -> &TypeAccess<TypeId> {
        &self.resource_access
    }

    fn is_non_send(&self) -> bool {
        false
    }

    unsafe fn run_unsafe(
        &mut self,
        _input: Self::In,
        _world: &World,
        _resources: &Resources,
    ) -> Option<Self::Out> {
        Some(if self.ran {
            ShouldRun::No
        } else {
            self.ran = true;
            ShouldRun::Yes
        })
    }

    fn apply_buffers(&mut self, _world: &mut World, _resources: &mut Resources) {}

    fn initialize(&mut self, _world: &mut World, _resources: &mut Resources) {}
}

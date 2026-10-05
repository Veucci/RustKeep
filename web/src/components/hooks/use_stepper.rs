use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum StepState {
    Completed,
    Active,
    Pending,
    Disabled,
}

#[derive(Clone)]
pub struct StepperContext {
    pub current_index: RwSignal<usize>,
    pub total_steps: usize,
    pub can_go_prev: Signal<bool>,
    pub can_go_next: Signal<bool>,
    pub go_next: Callback<(), ()>,
    pub go_prev: Callback<(), ()>,
    pub go_to: Callback<usize, ()>,
    pub step_state: Callback<usize, StepState>,
}

pub fn use_stepper(total_steps: usize, default_index: usize) -> StepperContext {

    let current_index = RwSignal::new(default_index.min(total_steps.saturating_sub(1)));

    let can_go_prev = Signal::derive(move || current_index.get() > 0);
    let can_go_next = Signal::derive(move || current_index.get() + 1 < total_steps);

    let go_prev = Callback::new(move |_| {
        if current_index.get() > 0 {
            current_index.update(|i| *i -= 1);
        }
    });

    let go_next = Callback::new(move |_| {
        if current_index.get() + 1 < total_steps {
            current_index.update(|i| *i += 1);
        }
    });

    let go_to = Callback::new(move |index: usize| {
        if index < total_steps {
            current_index.set(index);
        }
    });

    let step_state = Callback::new(move |step: usize| {
        let current = current_index.get();

        match step.cmp(&current) {
            std::cmp::Ordering::Less => StepState::Completed,
            std::cmp::Ordering::Equal => StepState::Active,
            std::cmp::Ordering::Greater => StepState::Pending,
        }
    });

    StepperContext { current_index, total_steps, can_go_prev, can_go_next, go_next, go_prev, go_to, step_state }
}

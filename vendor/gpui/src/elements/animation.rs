use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use crate::{
    AnyElement, App, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, Timer,
    Window,
};

pub use easing::*;
use smallvec::SmallVec;

/// Shared per-window pending re-arms for throttled animations (see
/// [`Animation::with_fps`]). At most one timer-driven redraw is scheduled per
/// window at any time, so multiple throttled animations merge into a single
/// shared frame cadence instead of multiplying the induced frame rate.
#[derive(Default)]
struct AnimationFrameThrottle {
    /// window handle -> instant the pending re-arm was scheduled for
    pending: std::collections::HashMap<crate::AnyWindowHandle, Instant>,
}

impl crate::Global for AnimationFrameThrottle {}

/// An animation that can be applied to an element.
#[derive(Clone)]
pub struct Animation {
    /// The amount of time for which this animation should run
    pub duration: Duration,
    /// Whether to repeat this animation when it finishes
    pub oneshot: bool,
    /// A function that takes a delta between 0 and 1 and returns a new delta
    /// between 0 and 1 based on the given easing function.
    pub easing: Rc<dyn Fn(f32) -> f32>,
    /// Optional throttle: minimum interval between animation frames requested
    /// by this animation. `None` (the default) requests a frame after every
    /// frame, which makes the animation run at the maximum frame rate the
    /// window can sustain. Continuous (`repeat()`) animations such as spinners
    /// should set this via [`Animation::with_fps`] to cap the window redraw
    /// rate they induce (each animation frame is a full window redraw).
    pub min_frame_interval: Option<Duration>,
}

impl Animation {
    /// Create a new animation with the given duration.
    /// By default the animation will only run once and will use a linear easing function.
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            oneshot: true,
            easing: Rc::new(linear),
            min_frame_interval: None,
        }
    }

    /// Throttle this animation to at most `fps` frames per second.
    ///
    /// Every animation frame triggers a full window redraw, so continuous
    /// (`repeat()`) animations without a cap burn an entire core redrawing the
    /// window as fast as it can. Small looping animations (spinners, marching
    /// dots) look smooth at 20-30fps; use this to cap the induced frame rate.
    pub fn with_fps(mut self, fps: f32) -> Self {
        self.min_frame_interval = Some(Duration::from_secs_f32(1.0 / fps.max(1.)));
        self
    }

    /// Set the animation to loop when it finishes.
    pub fn repeat(mut self) -> Self {
        self.oneshot = false;
        self
    }

    /// Set the easing function to use for this animation.
    /// The easing function will take a time delta between 0 and 1 and return a new delta
    /// between 0 and 1
    pub fn with_easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Rc::new(easing);
        self
    }
}

/// An extension trait for adding the animation wrapper to both Elements and Components
pub trait AnimationExt {
    /// Render this component or element with an animation
    fn with_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(move |this, _, value| animator(this, value)),
            animations: smallvec::smallvec![animation],
        }
    }

    /// Render this component or element with a chain of animations
    fn with_animations(
        self,
        id: impl Into<ElementId>,
        animations: Vec<Animation>,
        animator: impl Fn(Self, usize, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(animator),
            animations: animations.into(),
        }
    }
}

impl<E: IntoElement + 'static> AnimationExt for E {}

/// A GPUI element that applies an animation to another element
pub struct AnimationElement<E> {
    id: ElementId,
    element: Option<E>,
    animations: SmallVec<[Animation; 1]>,
    animator: Box<dyn Fn(E, usize, f32) -> E + 'static>,
}

impl<E> AnimationElement<E> {
    /// Returns a new [`AnimationElement<E>`] after applying the given function
    /// to the element being animated.
    pub fn map_element(mut self, f: impl FnOnce(E) -> E) -> AnimationElement<E> {
        self.element = self.element.map(f);
        self
    }
}

impl<E: IntoElement + 'static> IntoElement for AnimationElement<E> {
    type Element = AnimationElement<E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct AnimationState {
    start: Instant,
    animation_ix: usize,
}

impl<E: IntoElement + 'static> Element for AnimationElement<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        window.with_element_state(global_id.unwrap(), |state, window| {
            let mut state = state.unwrap_or_else(|| AnimationState {
                start: Instant::now(),
                animation_ix: 0,
            });
            let animation_ix = state.animation_ix;

            let mut delta = state.start.elapsed().as_secs_f32()
                / self.animations[animation_ix].duration.as_secs_f32();

            let mut done = false;
            if delta > 1.0 {
                if self.animations[animation_ix].oneshot {
                    if animation_ix >= self.animations.len() - 1 {
                        done = true;
                    } else {
                        state.start = Instant::now();
                        state.animation_ix += 1;
                    }
                    delta = 1.0;
                } else {
                    delta %= 1.0;
                }
            }
            let delta = (self.animations[animation_ix].easing)(delta);

            debug_assert!(
                (0.0..=1.0).contains(&delta),
                "delta should always be between 0 and 1"
            );

            let element = self.element.take().expect("should only be called once");
            let mut element = (self.animator)(element, animation_ix, delta).into_any_element();

            if !done {
                match self.animations[animation_ix].min_frame_interval {
                    // Re-arm at most once per `min_interval`, and share a single
                    // pending re-arm per window across all throttled animations,
                    // so N concurrent spinners still induce at most one frame
                    // per interval instead of N. External frames (input,
                    // streaming updates) still redraw at any rate and sample the
                    // animation whenever they happen, since delta is computed
                    // from `state.start`.
                    Some(min_interval) => {
                        let handle = window.window_handle();
                        let entity = window.current_view();
                        let throttle = cx.default_global::<AnimationFrameThrottle>();
                        if !throttle.pending.contains_key(&handle) {
                            throttle.pending.insert(handle, Instant::now() + min_interval);
                            cx.spawn(async move |cx| {
                                Timer::after(min_interval).await;
                                let _ = handle.update(cx, |_root_view, _window, cx| {
                                    cx.default_global::<AnimationFrameThrottle>()
                                        .pending
                                        .remove(&handle);
                                    cx.notify(entity);
                                });
                            })
                            .detach();
                        }
                    }
                    None => window.request_animation_frame(),
                }
            }

            ((element.request_layout(window, cx), element), state)
        })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

mod easing {
    use std::f32::consts::PI;

    /// The linear easing function, or delta itself
    pub fn linear(delta: f32) -> f32 {
        delta
    }

    /// The quadratic easing function, delta * delta
    pub fn quadratic(delta: f32) -> f32 {
        delta * delta
    }

    /// The quadratic ease-in-out function, which starts and ends slowly but speeds up in the middle
    pub fn ease_in_out(delta: f32) -> f32 {
        if delta < 0.5 {
            2.0 * delta * delta
        } else {
            let x = -2.0 * delta + 2.0;
            1.0 - x * x / 2.0
        }
    }

    /// The Quint ease-out function, which starts quickly and decelerates to a stop
    pub fn ease_out_quint() -> impl Fn(f32) -> f32 {
        move |delta| 1.0 - (1.0 - delta).powi(5)
    }

    /// Apply the given easing function, first in the forward direction and then in the reverse direction
    pub fn bounce(easing: impl Fn(f32) -> f32) -> impl Fn(f32) -> f32 {
        move |delta| {
            if delta < 0.5 {
                easing(delta * 2.0)
            } else {
                easing((1.0 - delta) * 2.0)
            }
        }
    }

    /// A custom easing function for pulsating alpha that slows down as it approaches 0.1
    pub fn pulsating_between(min: f32, max: f32) -> impl Fn(f32) -> f32 {
        let range = max - min;

        move |delta| {
            // Use a combination of sine and cubic functions for a more natural breathing rhythm
            let t = (delta * 2.0 * PI).sin();
            let breath = (t * t * t + t) / 2.0;

            // Map the breath to our desired alpha range
            let normalized_alpha = (breath + 1.0) / 2.0;

            min + (normalized_alpha * range)
        }
    }
}

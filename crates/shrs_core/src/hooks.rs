//! Shell runtime hooks
//!
//! Hooks are user provided functions that are called on a variety of events that occur in the
//! shell. Some additional context is provided to these hooks.
// ideas for hooks
// - on start
// - after prompt
// - before prompt
// - internal error hook (call whenever there is internal shell error; good for debug)
// - env hook (when environment variable is set/changed)
// - exit hook (tricky, make sure we know what cases to call this)

use std::marker::PhantomData;

use anyhow::Result;
use log::warn;

use crate::{
    hook_ctx::HookCtx,
    prelude::{Shell, States},
    state::Param,
};

impl<F, C: HookCtx> Hook<C> for FunctionHook<(Shell, C), F>
where
    for<'a, 'b> &'a F: Fn(&Shell, &C) -> Result<()>,
{
    fn run(&self, sh: &Shell, _states: &States, c: &C) -> Result<()> {
        fn call_inner<C: HookCtx>(
            f: impl Fn(&Shell, &C) -> Result<()>,
            sh: &Shell,
            states: &C,
        ) -> Result<()> {
            f(&sh, &states)
        }

        call_inner(&self.f, sh, c)
    }
}

macro_rules! impl_hook{
    (
        $($params:ident),+
    ) => {
        #[allow(non_snake_case)]
        #[allow(unused)]
        impl<F, C:HookCtx,$($params: Param),+> Hook<C> for FunctionHook<($($params),+,C), F>
            where
                for<'a, 'b> &'a F:
                    Fn( $($params),+,&Shell,&C ) ->Result<()>+
                    Fn( $(<$params as Param>::Item<'b>),+,&Shell,&C )->Result<()>
        {
            fn run(&self, sh:&Shell,states: &States, c: &C)->Result<()> {
                fn call_inner<C:HookCtx,$($params),+>(
                    f: impl Fn($($params),+,&Shell,&C)->Result<()>,
                    $($params: $params),+
                    ,sh:&Shell
                    ,states:&C
                ) ->Result<()>{
                    f($($params),+,sh,&states)
                }

                $(
                    let $params = $params::retrieve(states);
                )+

                call_inner(&self.f, $($params),+,sh,c)
            }
        }
    }
}

impl<F, C: HookCtx> IntoHook<(), C> for F
where
    for<'a, 'b> &'a F: Fn(&Shell, &C) -> Result<()>,
{
    type Hook = FunctionHook<(Shell, C), Self>;

    fn into_hook(self) -> Self::Hook {
        FunctionHook {
            f: self,
            marker: Default::default(),
        }
    }
}
impl<C: HookCtx, H: Hook<C>> IntoHook<H, C> for H {
    type Hook = H;

    fn into_hook(self) -> Self::Hook {
        self
    }
}

macro_rules! impl_into_hook {
    (
        $($params:ident),+
    ) => {
        impl<F, C:HookCtx,$($params: Param),+> IntoHook<($($params,)+),C> for F
            where
                for<'a, 'b> &'a F:
                    Fn( $($params),+,&Shell,&C ) ->Result<()>+
                    Fn( $(<$params as Param>::Item<'b>),+,&Shell,&C )->Result<()>
        {
            type Hook = FunctionHook<($($params,)+C), Self>;

            fn into_hook(self) -> Self::Hook {
                FunctionHook {
                    f: self,
                    marker: Default::default(),
                }
            }
        }
    }
}

pub struct FunctionHook<Input, F> {
    f: F,
    marker: PhantomData<fn() -> Input>,
}

pub trait Hook<C: HookCtx> {
    fn run(&self, sh: &Shell, states: &States, ctx: &C) -> Result<()>;
}
impl_hook!(T1);
impl_hook!(T1, T2);
impl_hook!(T1, T2, T3);
impl_hook!(T1, T2, T3, T4);
impl_hook!(T1, T2, T3, T4, T5);

pub trait IntoHook<Input, C: HookCtx> {
    type Hook: Hook<C>;

    fn into_hook(self) -> Self::Hook;
}

impl_into_hook!(T1);
impl_into_hook!(T1, T2);
impl_into_hook!(T1, T2, T3);
impl_into_hook!(T1, T2, T3, T4);
impl_into_hook!(T1, T2, T3, T4, T5);

pub type StoredHook<C> = Box<dyn Hook<C>>;

#[derive(Default)]
pub struct Hooks {
    hooks: anymap::Map,
}

impl Hooks {
    pub fn new() -> Self {
        Self {
            hooks: anymap::Map::new(),
        }
    }

    // TODO currently this will abort if a hook fails, potentially introduce fail modes like
    // 'Best Effort' - run all hooks and report any failures
    // 'Pedantic' - abort on the first failed hook
    pub(crate) fn run<C: HookCtx>(&self, sh: &Shell, states: &States, c: &C) -> Result<()> {
        if let Some(hook_list) = self.get::<C>() {
            for hook in hook_list.iter() {
                if let Err(e) = hook.run(sh, states, c) {
                    let type_name = std::any::type_name::<C>();
                    warn!("failed to execute hook {e} of type {type_name}");
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    pub fn insert<I, C: HookCtx, S: Hook<C> + 'static>(
        &mut self,
        hook: impl IntoHook<I, C, Hook = S>,
    ) {
        let h = Box::new(hook.into_hook());
        match self.hooks.get_mut::<Vec<StoredHook<C>>>() {
            Some(hook_list) => {
                hook_list.push(h);
            },
            None => {
                // register any empty vector for the type
                self.hooks.insert::<Vec<StoredHook<C>>>(vec![h]);
            },
        };
    }

    /// gets hooks associated with Ctx
    pub fn get<C: HookCtx>(&self) -> Option<&Vec<Box<dyn Hook<C>>>> {
        self.hooks.get::<Vec<StoredHook<C>>>()
    }
}

/// A session has separate cutoffs for opening the target and using it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SessionDeadline<T> {
    Opening(Option<T>),
    Active(Option<T>),
}

impl<T: Copy + Ord> SessionDeadline<T> {
    pub(super) fn at(self) -> Option<T> {
        match self {
            Self::Opening(at) | Self::Active(at) => at,
        }
    }

    pub(super) fn phase(self) -> &'static str {
        match self {
            Self::Opening(_) => "opening",
            Self::Active(_) => "active",
        }
    }

    pub(super) fn on_connected<E>(
        self,
        observed_at: T,
        active_cutoff: impl FnOnce(T) -> Result<Option<T>, E>,
    ) -> Result<Option<Self>, E> {
        if self.at().is_some_and(|at| observed_at >= at) {
            return Ok(None);
        }
        match self {
            Self::Opening(_) => active_cutoff(observed_at).map(|at| Some(Self::Active(at))),
            Self::Active(_) => Ok(Some(self)),
        }
    }
}

//! Declarative macros for quickly constructing observable state.

/// Creates a new [`ObservableValue`](crate::observable::value::ObservableValue) with a derived name.
#[macro_export]
macro_rules! mobx_observable {
    ($name:ident : $ty:ty = $value:expr) => {{
        let __name = stringify!($name);
        let __value: $ty = $value;
        $crate::observable::value::ObservableValue::<$ty>::new(__name, __value)
    }};
    ($name:ident = $value:expr) => {{
        let __name = stringify!($name);
        let __value = $value;
        $crate::observable::value::ObservableValue::new(__name, __value)
    }};
    ($name:expr, $value:expr) => {{
        let __name = $name;
        let __value = $value;
        $crate::observable::value::ObservableValue::new(__name, __value)
    }};
}

/// Builds a [`Computed`](crate::Computed) value with optional setter support.
#[macro_export]
macro_rules! mobx_computed {
    ($name:ident = $getter:expr $(, setter = $setter:expr)? ) => {{
        let mut __options = $crate::ComputedOptions::new($getter).name(stringify!($name));
        $(
            __options = __options.setter($setter);
        )?
        $crate::Computed::new(__options)
    }};
    ($name:expr, $getter:expr $(, setter = $setter:expr)? ) => {{
        let mut __options = $crate::ComputedOptions::new($getter).name($name);
        $(
            __options = __options.setter($setter);
        )?
        $crate::Computed::new(__options)
    }};
}

/// Wraps a block or closure inside [`action`](crate::action) with an inferred name.
#[macro_export]
macro_rules! mobx_action {
    ($name:ident => $body:block) => {{ $crate::action(stringify!($name), || $body) }};
    ($name:expr => $body:block) => {{ $crate::action($name, || $body) }};
    ($name:ident, $closure:expr) => {{ $crate::action(stringify!($name), $closure) }};
    ($name:expr, $closure:expr) => {{ $crate::action($name, $closure) }};
}

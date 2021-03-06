use crate::KernelObjectId;
use bit_field::BitField;
use core::convert::TryFrom;

pub(super) macro define_error_type($error_name:ident {
    $($(#[$attrib:meta])*$name:ident => $repr_num:expr),*$(,)?
}) {
    #[derive(Clone, Copy, Debug)]
    pub enum $error_name {
        $(
            $(#[$attrib])*
            $name,
         )*
    }

    impl TryFrom<usize> for $error_name {
        type Error = ();

        fn try_from(status: usize) -> Result<Self, Self::Error> {
            match status {
                $(
                    $repr_num => Ok(Self::$name),
                 )*
                _ => Err(()),
            }
        }
    }

    impl Into<usize> for $error_name {
        fn into(self) -> usize {
            match self {
                $(
                    Self::$name => $repr_num,
                 )*
            }
        }
    }
}

pub fn status_from_syscall_repr<E>(status: usize) -> Result<(), E>
where
    E: TryFrom<usize, Error = ()>,
{
    if status == 0 {
        Ok(())
    } else {
        Err(E::try_from(status).expect("System call returned invalid status"))
    }
}

pub fn status_to_syscall_repr<E>(result: Result<(), E>) -> usize
where
    E: Into<usize>,
{
    match result {
        Ok(()) => 0,
        Err(err) => err.into(),
    }
}

pub fn result_from_syscall_repr<E>(result: usize) -> Result<KernelObjectId, E>
where
    E: TryFrom<usize, Error = ()>,
{
    let status = result.get_bits(32..64);
    if status == 0 {
        Ok(KernelObjectId::from_syscall_repr(result))
    } else {
        Err(E::try_from(status).expect("System call returned invalid result status"))
    }
}

pub fn result_to_syscall_repr<E>(result: Result<KernelObjectId, E>) -> usize
where
    E: Into<usize>,
{
    match result {
        Ok(id) => KernelObjectId::to_syscall_repr(id),
        Err(err) => {
            let mut value = 0usize;
            value.set_bits(32..64, err.into());
            value
        }
    }
}

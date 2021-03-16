use std::{convert::TryInto, io};

use uuid::Uuid;

pub(crate) fn u128_to_i64(i: u128) -> io::Result<i64> {
    let conversion: Result<i64, std::num::TryFromIntError> = i.try_into();
    match conversion {
        Ok(delta) => Ok(-delta),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}

pub(crate) fn uuid() -> String {
    Uuid::new_v4().to_string()
}

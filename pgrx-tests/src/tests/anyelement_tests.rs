use pgrx::{prelude::*, AnyElement};

#[pg_extern]
fn anyelement_arg(element: AnyElement) -> AnyElement {
    element
}

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    #[allow(unused_imports)]
    use crate as pgrx_tests;

    use pgrx::{prelude::*, AnyElement};

    #[pg_test]
    fn test_anyelement_arg() -> Result<(), pgrx::spi::Error> {
        let element = Spi::get_one_with_args::<AnyElement>(
            "SELECT anyelement_arg($1);",
            &[(PgBuiltInOids::ANYELEMENTOID.oid(), 123.into_datum())],
        )?
        .map(|e| e.datum());

        assert_eq!(element, 123.into_datum());

        Ok(())
    }
}

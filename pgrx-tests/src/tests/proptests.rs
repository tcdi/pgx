use pgrx::prelude::*;
use proptest::prelude::*;
use proptest::strategy::Strategy;
use crate::proptest::PgTestRunner;

#[pg_extern]
pub fn nop_date(date: Date) -> Date {
    date
}

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use super::*;
    use crate as pgrx_tests;

    // Property tests consist of 1:
    /// Hypothesis: We can pass random dates directly into Postgres functions and get them back.
    #[pg_test]
    pub fn date_spi_roundtrip() {
        // 2. Constructing the Postgres-adapted test runner
        let mut proptest = PgTestRunner::default();
        // 3. A strategy for creating and refining values
        let strat = prop::num::i32::ANY.prop_map_into::<Date>();
        // 4. The runner invocation
        proptest
            .run(&strat, |date| {
                let spi_ret: Date = Spi::get_one_with_args(
                    "SELECT nop_date($1)",
                    vec![(PgBuiltInOids::DATEOID.into(), date.into_datum())],
                )
                .unwrap()
                .unwrap();

                // 5. A condition on which the test is accepted or rejected
                prop_assert_eq!(date, spi_ret);
                Ok(())
            })
            .unwrap();
    }

    // Proptest's "trophy case" for pgrx includes:
    // Demonstrating that existing infallible functions can have fallible results when their code
    // is actually put in contact with the database
    /// Hypothesis: We can ask Postgres to accept any i32 as a Date, then print its value,
    /// and then get the same i32 back after passing it through SPI as a date literal
    /// Fails on:
    /// - date values between (non-inclusive) i32::MIN and -2451545
    /// - date values between (non-inclusive) i32::MAX and (2147483494 - 2451545) - 1
    #[pg_test]
    pub fn date_literal_spi_roundtrip() {
        let mut proptest = PgTestRunner::default();
        let strat = prop::num::i32::ANY.prop_map_into::<Date>();
        proptest
            .run(&strat, |date| {
                let datum = date.into_datum();
                let date_cstr: &std::ffi::CStr =
                    unsafe { pgrx::direct_function_call(pg_sys::date_out, &[datum]).unwrap() };
                let date_text = date_cstr.to_str().unwrap().to_owned();
                let spi_select_command = format!("SELECT nop_date('{}')", date_text);
                let spi_ret: Option<Date> = Spi::get_one(&spi_select_command).unwrap();
                prop_assert_eq!(date, spi_ret.unwrap());
                Ok(())
            })
            .unwrap();
    }
}

// struct TimeValueTree {}
// struct TimestampValueTree {}
// struct TimestampWithTimezoneValueTree {}

// fn create_array_sql_repr() -> ! {

// }

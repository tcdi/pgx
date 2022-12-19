/*
Portions Copyright 2019-2021 ZomboDB, LLC.
Portions Copyright 2021-2022 Technology Concepts & Design, Inc. <support@tcdi.com>

All rights reserved.

Use of this source code is governed by the MIT license that can be found in the LICENSE file.
*/

#[cfg(any(test, feature = "pg_test"))]
#[pgx::pg_schema]
mod tests {
    #[allow(unused_imports)]
    use crate as pgx_tests;

    use pgx::prelude::*;
    use pgx::spi;

    #[pg_test(error = "syntax error at or near \"THIS\"")]
    fn test_spi_failure() {
        Spi::execute(|client| {
            client.select("THIS IS NOT A VALID QUERY", None, None);
        });
    }

    #[pg_test]
    fn test_spi_can_nest() {
        Spi::execute(|_| {
            Spi::execute(|_| {
                Spi::execute(|_| {
                    Spi::execute(|_| {
                        Spi::execute(|_| {});
                    });
                });
            });
        });
    }

    #[pg_test]
    fn test_spi_returns_primitive() -> Result<(), spi::Error> {
        let rc = Spi::connect(|client| {
            client.select("SELECT 42", None, None).first().get_datum::<i32>(1)
        })?;

        assert_eq!(42, rc.expect("SPI failed to return proper value"));
        Ok(())
    }

    #[pg_test]
    fn test_spi_returns_str() -> Result<(), spi::Error> {
        let rc = Spi::connect(|client| {
            client.select("SELECT 'this is a test'", None, None).first().get_datum::<&str>(1)
        })?;

        assert_eq!("this is a test", rc.expect("SPI failed to return proper value"));
        Ok(())
    }

    #[pg_test]
    fn test_spi_returns_string() -> Result<(), spi::Error> {
        let rc = Spi::connect(|client| {
            client.select("SELECT 'this is a test'", None, None).first().get_datum::<String>(1)
        })?;

        assert_eq!("this is a test", rc.expect("SPI failed to return proper value"));
        Ok(())
    }

    #[pg_test]
    fn test_spi_get_one() {
        Spi::execute(|client| {
            let i = client
                .select("SELECT 42::bigint", None, None)
                .first()
                .get_one::<i64>()
                .expect("SPI failed");
            assert_eq!(Some(42), i);
        });
    }

    #[pg_test]
    fn test_spi_get_two() {
        Spi::execute(|client| {
            let (i, s) = client
                .select("SELECT 42, 'test'", None, None)
                .first()
                .get_two::<i64, &str>()
                .expect("SPI failed");

            assert_eq!(Some(42), i);
            assert_eq!(Some("test"), s);
        });
    }

    #[pg_test]
    fn test_spi_get_three() {
        Spi::execute(|client| {
            let (i, s, b) = client
                .select("SELECT 42, 'test', true", None, None)
                .first()
                .get_three::<i64, &str, bool>()
                .expect("SPI failed");

            assert_eq!(Some(42), i);
            assert_eq!(Some("test"), s);
            assert_eq!(Some(true), b);
        });
    }

    #[pg_test]
    fn test_spi_get_two_with_failure() {
        Spi::execute(|client| {
            assert!(client.select("SELECT 42", None, None).first().get_two::<i64, &str>().is_err());
        });
    }

    #[pg_test]
    fn test_spi_get_three_failure() {
        Spi::execute(|client| {
            assert!(client
                .select("SELECT 42, 'test'", None, None)
                .first()
                .get_three::<i64, &str, bool>()
                .is_err());
        });
    }

    #[pg_test]
    fn test_spi_select_zero_rows() {
        assert!(Spi::get_one::<i32>("SELECT 1 LIMIT 0").is_err());
    }

    #[pg_test]
    fn test_spi_run() {
        Spi::run("SELECT 1")
    }

    #[pg_test]
    fn test_spi_run_with_args() {
        let i = 1 as i32;
        let j = 2 as i64;

        Spi::run_with_args(
            "SELECT $1 + $2 = 3",
            Some(vec![
                (PgBuiltInOids::INT4OID.oid(), Some(i.into())),
                (PgBuiltInOids::INT8OID.oid(), Some(j.into())),
            ]),
        )
    }

    #[pg_test]
    fn test_spi_explain() -> Result<(), pgx::spi::Error> {
        let result = Spi::explain("SELECT 1")?;
        assert!(result.0.get(0).unwrap().get("Plan").is_some());
        Ok(())
    }

    #[pg_test]
    fn test_spi_explain_with_args() -> Result<(), pgx::spi::Error> {
        let i = 1 as i32;
        let j = 2 as i64;

        let result = Spi::explain_with_args(
            "SELECT $1 + $2 = 3",
            Some(vec![
                (PgBuiltInOids::INT4OID.oid(), Some(i.into())),
                (PgBuiltInOids::INT8OID.oid(), Some(j.into())),
            ]),
        )?;

        assert!(result.0.get(0).unwrap().get("Plan").is_some());
        Ok(())
    }

    #[pg_extern]
    fn do_panic() {
        panic!("did a panic");
    }

    #[pg_test(error = "did a panic")]
    fn test_panic_via_spi() {
        Spi::run("SELECT tests.do_panic();");
    }

    #[pg_test]
    fn test_inserting_null() -> Result<(), pgx::spi::Error> {
        Spi::execute(|client| {
            client.update("CREATE TABLE tests.null_test (id uuid)", None, None);
        });
        assert_eq!(
            Spi::get_one_with_args::<i32>(
                "INSERT INTO tests.null_test VALUES ($1) RETURNING 1",
                vec![(PgBuiltInOids::UUIDOID.oid(), None)],
            )?
            .unwrap(),
            1
        );
        Ok(())
    }

    #[pg_test]
    fn test_cursor() {
        Spi::execute(|client| {
            client.update("CREATE TABLE tests.cursor_table (id int)", None, None);
            client.update(
                "INSERT INTO tests.cursor_table (id) \
            SELECT i FROM generate_series(1, 10) AS t(i)",
                None,
                None,
            );
            let mut portal = client.open_cursor("SELECT * FROM tests.cursor_table", None).unwrap();

            fn sum_all(table: pgx::SpiTupleTable) -> i32 {
                table.map(|r| r.by_ordinal(1).unwrap().value::<i32>().unwrap()).sum()
            }
            assert_eq!(sum_all(portal.fetch(3)), 1 + 2 + 3);
            assert_eq!(sum_all(portal.fetch(3)), 4 + 5 + 6);
            assert_eq!(sum_all(portal.fetch(3)), 7 + 8 + 9);
            assert_eq!(sum_all(portal.fetch(3)), 10);
        });
    }

    #[pg_test]
    fn test_cursor_by_name() -> Result<(), pgx::spi::Error> {
        let cursor_name = Spi::connect(|client| {
            client.update("CREATE TABLE tests.cursor_table (id int)", None, None);
            client.update(
                "INSERT INTO tests.cursor_table (id) \
            SELECT i FROM generate_series(1, 10) AS t(i)",
                None,
                None,
            );
            client.open_cursor("SELECT * FROM tests.cursor_table", None).map(|mut cursor| {
                assert_eq!(sum_all(cursor.fetch(3)), 1 + 2 + 3);
                cursor.detach_into_name()
            })
        })?;

        fn sum_all(table: pgx::SpiTupleTable) -> i32 {
            table.map(|r| r.by_ordinal(1).unwrap().value::<i32>().unwrap()).sum()
        }
        Spi::connect(|client| {
            client.find_cursor(&cursor_name).map(|mut cursor| {
                assert_eq!(sum_all(cursor.fetch(3)), 4 + 5 + 6);
                assert_eq!(sum_all(cursor.fetch(3)), 7 + 8 + 9);
                cursor.detach_into_name();
            })
        })?;

        Spi::connect(|client| {
            client.find_cursor(&cursor_name).map(|mut cursor| {
                assert_eq!(sum_all(cursor.fetch(3)), 10);
            })
        })?;
        Ok(())
    }

    #[pg_test(error = "syntax error at or near \"THIS\"")]
    fn test_cursor_failure() {
        Spi::connect(|client| client.open_cursor("THIS IS NOT SQL", None).map(|_| ())).unwrap();
    }

    #[pg_test(error = "cursor: CursorNotFound(\"NOT A CURSOR\")")]
    fn test_cursor_not_found() {
        Spi::connect(|client| client.find_cursor("NOT A CURSOR").map(|_| ())).expect("cursor");
    }

    #[pg_test]
    fn test_columns() {
        use pgx::{PgBuiltInOids, PgOid};
        Spi::execute(|client| {
            let res = client.select("SELECT 42 AS a, 'test' AS b", None, None);

            assert_eq!(2, res.columns());

            assert_eq!(res.column_type_oid(1).unwrap(), PgOid::BuiltIn(PgBuiltInOids::INT4OID));

            assert_eq!(res.column_type_oid(2).unwrap(), PgOid::BuiltIn(PgBuiltInOids::TEXTOID));

            assert_eq!(res.column_name(1).unwrap(), "a");

            assert_eq!(res.column_name(2).unwrap(), "b");
        });

        Spi::execute(|client| {
            let res = client.update("SET TIME ZONE 'PST8PDT'", None, None);

            assert_eq!(0, res.columns());
        });
    }

    #[pg_test]
    fn test_connect_return_anything() {
        struct T;
        assert!(matches!(Spi::connect(|_| Ok::<_, ()>(Some(T))).unwrap().unwrap(), T));
    }

    #[pg_test]
    fn test_spi_non_mut() -> Result<(), pgx::spi::Error> {
        // Ensures update and cursor APIs do not need mutable reference to SpiClient
        Spi::connect(|client| {
            client.update("SELECT 1", None, None);
            let cursor = client.open_cursor("SELECT 1", None)?.detach_into_name();
            client.find_cursor(&cursor).map(|_| ())
        })
    }

    #[pg_test]
    fn test_open_multiple_tuptables() {
        Spi::execute(|client| {
            let a = client.select("SELECT 1", None, None).first();
            let _b = client.select("SELECT 1 WHERE 'f'", None, None);
            assert!(!a.is_empty());
            assert_eq!(1, a.len());
            assert!(a.get_heap_tuple().is_some());
            assert_eq!(1, a.get_datum::<i32>(1).expect("a.get_datum::<i32>(1) failed").unwrap());
        });
    }

    #[pg_test]
    #[ignore = "come back to this test"]
    fn test_open_multiple_tuptables_rev() {
        Spi::execute(|client| {
            let a = client.select("SELECT 1 WHERE 'f'", None, None).first();
            let _b = client.select("SELECT 1", None, None);
            assert!(a.is_empty());
            assert_eq!(0, a.len());
            assert!(a.get_heap_tuple().is_none());
            assert!(a.get_datum::<i32>(1).expect("a.get_datum::<i32>(1) failed").is_none());
        });
    }

    #[pg_test]
    fn test_spi_unwind_safe() {
        struct T;
        assert!(matches!(Spi::connect(|_| Ok::<_, ()>(Some(T))).unwrap().unwrap(), T));
    }

    #[pg_test]
    fn test_error_propagation() {
        #[derive(Debug)]
        struct Error;
        let result = Spi::connect(|_| Err::<(), _>(Error));
        assert!(matches!(result, Err(Error)))
    }

    #[pg_test]
    fn test_option() {
        assert!(Spi::get_one::<i32>("SELECT NULL::integer").unwrap().is_none());
    }
}

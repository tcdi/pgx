//LICENSE Portions Copyright 2019-2021 ZomboDB, LLC.
//LICENSE
//LICENSE Portions Copyright 2021-2023 Technology Concepts & Design, Inc.
//LICENSE
//LICENSE Portions Copyright 2023-2023 PgCentral Foundation, Inc. <contact@pgcentral.org>
//LICENSE
//LICENSE All rights reserved.
//LICENSE
//LICENSE Use of this source code is governed by the MIT license that can be found in the LICENSE file.
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    #[allow(unused_imports)]
    use crate as pgrx_tests;

    use pgrx::fcall::*;
    use pgrx::prelude::*;

    #[pg_extern]
    fn my_int4eq(l: i32, r: i32) -> bool {
        l == r
    }

    #[pg_extern]
    fn arg_might_be_null(v: Option<i32>) -> Option<i32> {
        v
    }

    extension_sql!(
        r#"
            CREATE FUNCTION tests.sql_int4eq(int4, int4) RETURNS bool STRICT LANGUAGE sql AS $$ SELECT $1 = $2; $$;
            
            CREATE FUNCTION tests.with_only_default(int4 DEFAULT 0) RETURNS text STRICT LANGUAGE sql AS $$ SELECT 'int4'; $$;
            CREATE FUNCTION tests.with_only_default(int8 DEFAULT 0) RETURNS text STRICT LANGUAGE sql AS $$ SELECT 'int8'; $$;
            
            CREATE FUNCTION tests.with_default(int4, int4 DEFAULT 0) RETURNS int4 STRICT LANGUAGE sql AS $$ SELECT $1 + $2; $$;
            
            CREATE FUNCTION tests.with_two_defaults(int4 DEFAULT 0, int4 DEFAULT 0) RETURNS int4 STRICT LANGUAGE sql AS $$ SELECT $1 + $2; $$;
            
            CREATE FUNCTION tests.with_arg_and_two_defaults(int4, int4 DEFAULT 1, int4 DEFAULT 2) RETURNS int4 STRICT LANGUAGE sql AS $$ SELECT $1 + $2 + $3; $$;
            
            CREATE FUNCTION tests.with_null_default(text DEFAULT NULL) RETURNS text STRICT LANGUAGE sql AS $$ SELECT $1; $$;
            
            CREATE FUNCTION tests.n() RETURNS text IMMUTABLE LANGUAGE sql AS $$ SELECT NULL; $$;
            CREATE FUNCTION tests.with_functional_default(text DEFAULT tests.n()) RETURNS text STRICT LANGUAGE sql AS $$ SELECT $1; $$;
        "#,
        name = "test_funcs",
        requires = [tests]
    );

    #[pg_test]
    fn test_int4eq_eq() {
        let result = fcall::<bool>("pg_catalog.int4eq", &[&Arg::Value(1), &Arg::Value(1)]);
        assert_eq!(Ok(Some(true)), result)
    }

    #[pg_test]
    fn test_int4eq_ne() {
        let result = fcall::<bool>("pg_catalog.int4eq", &[&Arg::Value(1), &Arg::Value(2)]);
        assert_eq!(Ok(Some(false)), result)
    }

    #[pg_test]
    fn test_my_int4eq() {
        let result = fcall::<bool>("tests.my_int4eq", &[&Arg::Value(1), &Arg::Value(1)]);
        assert_eq!(Ok(Some(true)), result)
    }

    #[pg_test]
    fn test_sql_int4eq() {
        let result = fcall::<bool>("tests.sql_int4eq", &[&Arg::Value(1), &Arg::Value(1)]);
        assert_eq!(Ok(Some(true)), result)
    }

    #[pg_test]
    fn test_null_arg_some() {
        let result = fcall::<i32>("tests.arg_might_be_null", &[&Arg::Value(1)]);
        assert_eq!(Ok(Some(1)), result)
    }

    #[pg_test]
    fn test_null_arg_none() {
        let result = fcall::<i32>("tests.arg_might_be_null", &[&Arg::<i32>::Null]);
        assert_eq!(Ok(None), result)
    }

    #[pg_test]
    fn test_strict() {
        // calling a STRICT function such as pg_catalog.float4 with a NULL argument will crash Postgres
        let result = fcall::<f32>("pg_catalog.float4", &[&Arg::<AnyNumeric>::Null]);
        assert_eq!(Ok(None), result);
    }

    #[pg_test]
    fn test_incompatible_return_type() {
        let result = fcall::<String>("pg_catalog.int4eq", &[&Arg::Value(1), &Arg::Value(1)]);
        assert_eq!(
            Err(FCallError::IncompatibleReturnType(String::type_oid(), pg_sys::BOOLOID)),
            result
        );
    }

    #[pg_test]
    fn test_too_many_args() {
        let args: [&dyn FCallArg; 32768] = [&Arg::Value(1); 32768];
        let result = fcall::<bool>("pg_catalog.int4eq", &args);
        assert_eq!(Err(FCallError::TooManyArguments), result);
    }

    #[pg_test]
    fn test_with_only_default_ambiguous() {
        let result = fcall::<&str>("tests.with_only_default", &[]);
        assert_eq!(Err(FCallError::AmbiguousFunction), result);
    }

    #[pg_test]
    fn test_with_only_default_int4() {
        let result = fcall::<&str>("tests.with_only_default", &[&Arg::<i32>::Default]);
        assert_eq!(Ok(Some("int4")), result);
    }

    #[pg_test]
    fn test_with_only_default_int8() {
        let result = fcall::<&str>("tests.with_only_default", &[&Arg::<i64>::Default]);
        assert_eq!(Ok(Some("int8")), result);
    }

    #[pg_test]
    fn test_with_default() {
        let result = fcall::<i32>("tests.with_default", &[&Arg::Value(42), &Arg::<i32>::Default]);
        assert_eq!(Ok(Some(42)), result);
    }

    #[pg_test]
    fn test_with_two_defaults() {
        let result = fcall::<i32>("tests.with_two_defaults", &[]);
        assert_eq!(Ok(Some(0)), result);

        let result = fcall::<i32>("tests.with_two_defaults", &[&Arg::Value(1)]);
        assert_eq!(Ok(Some(1)), result);

        let result = fcall::<i32>("tests.with_two_defaults", &[&Arg::Value(1), &Arg::Value(1)]);
        assert_eq!(Ok(Some(2)), result);
    }

    #[pg_test]
    fn test_with_arg_and_two_defaults_no_args() {
        let result = fcall::<i32>("tests.with_arg_and_two_defaults", &[]);
        assert_eq!(Err(FCallError::NotDefaultArgument(0)), result);
    }

    #[pg_test]
    fn test_with_arg_and_two_defaults_no_args_1_arg() {
        let result = fcall::<i32>("tests.with_arg_and_two_defaults", &[&Arg::Value(42)]);
        assert_eq!(Ok(Some(45)), result);
    }

    #[pg_test]
    fn test_with_arg_and_two_defaults_no_args_2_args() {
        let result =
            fcall::<i32>("tests.with_arg_and_two_defaults", &[&Arg::Value(42), &Arg::Value(0)]);
        assert_eq!(Ok(Some(44)), result);
    }

    #[pg_test]
    fn test_with_null_default() {
        let result = fcall::<&str>("tests.with_null_default", &[]);
        assert_eq!(Ok(None), result);

        let result = fcall::<&str>("tests.with_null_default", &[&Arg::Value("value")]);
        assert_eq!(Ok(Some("value")), result);
    }

    #[pg_test]
    fn test_with_functional_default() {
        let result = fcall::<&str>("tests.with_functional_default", &[]);
        assert_eq!(Ok(None), result);

        let result = fcall::<&str>("tests.with_functional_default", &[&Arg::Value("value")]);
        assert_eq!(Ok(Some("value")), result);
    }

    #[pg_test]
    fn test_with_unspecified_default() {
        let result = fcall::<i32>("tests.with_default", &[&Arg::Value(42)]);
        assert_eq!(Ok(Some(42)), result);
    }

    #[pg_test]
    fn test_func_with_collation() {
        // `pg_catalog.texteq` requires a collation, so we use the default
        let result =
            fcall::<bool>("pg_catalog.texteq", &[&Arg::Value("test"), &Arg::Value("test")]);
        assert_eq!(Ok(Some(true)), result);
    }

    #[pg_test]
    fn unknown_function() {
        let result = fcall::<()>("undefined_function", &[]);
        assert_eq!(Err(FCallError::UndefinedFunction), result)
    }

    #[pg_test]
    fn blank_function() {
        let result = fcall::<()>("", &[]);
        assert_eq!(Err(FCallError::InvalidIdentifier(String::from(""))), result)
    }

    #[pg_test]
    fn invalid_identifier() {
        let stupid_name = "q234qasf )(A*q2342";
        let result = fcall::<()>(stupid_name, &[]);
        assert_eq!(Err(FCallError::InvalidIdentifier(String::from(stupid_name))), result)
    }
}

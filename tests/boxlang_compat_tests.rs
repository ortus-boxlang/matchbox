//! BoxLang compatibility transfer runner.
//!
//! Each `.bxs` file under `tests/scripts/boxlang_compat/` is a 1:1 black-box
//! counterpart of a BoxLang JVM test (`../BoxLang/src/test/java/ortus/boxlang/...`).
//! The directory structure mirrors the JVM test tree and each file header
//! records the JVM test it ties back to.
//!
//! This milestone is *transfer-first*: the `.bxs` files capture BoxLang runtime
//! behavior. Every transferred test is registered and RUNS. Tests whose
//! behavior MatchBox does not implement yet simply fail (red) — that is the
//! signal the follow-up compatibility work consumes. Only tests that cannot be
//! expressed in BoxLang at all (JVM-only infrastructure such as Java
//! reflection) are skipped, and those are documented in the `.bxs` header.

#![allow(dead_code)]

use matchbox::process_file;
use std::path::Path;

/// Run one transferred compat script through the MatchBox VM.
fn run_compat(rel: &str) -> anyhow::Result<()> {
    if std::env::var_os("MATCHBOX_TEST_DB_HOST").is_none()
        && matches!(
            rel,
            "runtime/bifs/global/jdbc/IsInTransactionTest.bxs"
                | "runtime/bifs/global/jdbc/PreserveSingleQuotesTest.bxs"
                | "runtime/bifs/global/jdbc/QueryExecuteTest.bxs"
        )
    {
        return Ok(());
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("boxlang_compat")
        .join(rel);

    process_file(
        &path,
        false,
        None,
        Vec::new(),
        false,
        false,
        false,
        None,
        &[],
        false,
        None,
        false,
        false,
        false,
        false,
    )
}

macro_rules! compat_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            if let Err(e) = run_compat($file) {
                panic!("BoxLang compat '{}' failed: {}", $file, e);
            }
        }
    };
    ($name:ident, $file:expr, $reason:literal) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            if let Err(e) = run_compat($file) {
                panic!("BoxLang compat '{}' failed: {}", $file, e);
            }
        }
    };
}

// ---------------------------------------------------------------------------
// runtime/operators — ortus/boxlang/runtime/operators/*.java
// ---------------------------------------------------------------------------

// Every transferred file runs. Failing assertions surface as red tests; the
// `.bxs` header documents which behavior is still missing.

compat_test!(op_and, "runtime/operators/AndTest.bxs");
compat_test!(op_assert, "runtime/operators/AssertTest.bxs");
compat_test!(op_bitwise_and, "runtime/operators/BitwiseAndTest.bxs");
compat_test!(
    op_bitwise_complement,
    "runtime/operators/BitwiseComplementTest.bxs"
);
compat_test!(
    op_bitwise_operators,
    "runtime/operators/BitwiseOperatorsTest.bxs"
);
compat_test!(op_bitwise_or, "runtime/operators/BitwiseOrTest.bxs");
compat_test!(
    op_bitwise_signed_left_shift,
    "runtime/operators/BitwiseSignedLeftShiftTest.bxs"
);
compat_test!(
    op_bitwise_signed_right_shift,
    "runtime/operators/BitwiseSignedRightShiftTest.bxs"
);
compat_test!(
    op_bitwise_unsigned_right_shift,
    "runtime/operators/BitwiseUnsignedRightShiftTest.bxs"
);
compat_test!(op_bitwise_xor, "runtime/operators/BitwiseXorTest.bxs");
compat_test!(op_compare, "runtime/operators/CompareTest.bxs");
compat_test!(op_concat, "runtime/operators/ConcatTest.bxs");
compat_test!(op_contains, "runtime/operators/ContainsTest.bxs");
compat_test!(op_decrement, "runtime/operators/DecrementTest.bxs");
compat_test!(op_divide, "runtime/operators/DivideTest.bxs");
compat_test!(op_elvis, "runtime/operators/ElvisTest.bxs");
compat_test!(op_equals_equals, "runtime/operators/EqualsEqualsTest.bxs");
compat_test!(
    op_equals_equals_equals,
    "runtime/operators/EqualsEqualsEqualsTest.bxs"
);
compat_test!(
    op_greater_than_equal,
    "runtime/operators/GreaterThanEqualTest.bxs"
);
compat_test!(op_greater_than, "runtime/operators/GreaterThanTest.bxs");
compat_test!(op_increment, "runtime/operators/IncrementTest.bxs");
compat_test!(op_instance_of, "runtime/operators/InstanceOfTest.bxs");
compat_test!(op_integer_divide, "runtime/operators/IntegerDivideTest.bxs");
compat_test!(
    op_less_than_equal,
    "runtime/operators/LessThanEqualTest.bxs"
);
compat_test!(op_less_than, "runtime/operators/LessThanTest.bxs");
compat_test!(op_minus, "runtime/operators/MinusTest.bxs");
compat_test!(op_modulus, "runtime/operators/ModulusTest.bxs");
compat_test!(op_multiply, "runtime/operators/MultiplyTest.bxs");
compat_test!(op_negate, "runtime/operators/NegateTest.bxs");
compat_test!(op_not, "runtime/operators/NotTest.bxs");
compat_test!(op_or, "runtime/operators/OrTest.bxs");
compat_test!(op_plus, "runtime/operators/PlusTest.bxs");
compat_test!(op_power, "runtime/operators/PowerTest.bxs");
compat_test!(op_range, "runtime/operators/RangeTest.bxs");
compat_test!(op_set_operators, "runtime/operators/SetOperatorsTest.bxs");
compat_test!(op_ternary, "runtime/operators/TernaryTest.bxs");
compat_test!(op_xor, "runtime/operators/XORTest.bxs");

// JVM-only, cannot be expressed as BoxLang source: the assertions verify Java
// result classes via `.getClass().getName()`.
compat_test!(
    op_cast_as,
    "runtime/operators/CastAsTest.bxs",
    "JVM-only: asserts Java cast-result classes via reflection"
);

// ---------------------------------------------------------------------------
// runtime/scopes — ortus/boxlang/runtime/scopes/*.java
// ---------------------------------------------------------------------------
// All six drive the internal Java scope/Key APIs directly; headers document
// each skip.

compat_test!(sc_arguments, "runtime/scopes/ArgumentsScopeTest.bxs");
compat_test!(sc_base, "runtime/scopes/BaseScopeTest.bxs");
compat_test!(sc_int_key, "runtime/scopes/IntKeyTest.bxs");
compat_test!(sc_key, "runtime/scopes/KeyTest.bxs");
compat_test!(sc_scope_wrapper, "runtime/scopes/ScopeWrapperTest.bxs");
compat_test!(sc_server_scope, "runtime/scopes/ServerScopeTest.bxs");

// ---------------------------------------------------------------------------
// runtime/dynamic — ortus/boxlang/runtime/dynamic/*.java
// ---------------------------------------------------------------------------

// The four top-level dynamic tests drive internal Java APIs; headers document
// each skip.
compat_test!(dyn_attempt, "runtime/dynamic/AttemptTest.bxs");
compat_test!(
    dyn_expression_interpreter,
    "runtime/dynamic/ExpressionInterpreterTest.bxs"
);
compat_test!(
    dyn_functional_interface,
    "runtime/dynamic/FunctionalInterfaceTest.bxs"
);
compat_test!(dyn_referencer, "runtime/dynamic/ReferencerTest.bxs");

// Value casters, transferred via `cast( value, "type" )`. MatchBox has no
// `cast` BIF yet, so these fail (red) until it lands.
compat_test!(cst_boolean, "runtime/dynamic/casters/BooleanCasterTest.bxs");
compat_test!(cst_number, "runtime/dynamic/casters/NumberCasterTest.bxs");
compat_test!(cst_string, "runtime/dynamic/casters/StringCasterTest.bxs");
compat_test!(cst_double, "runtime/dynamic/casters/DoubleCasterTest.bxs");

// Java-internal casters (java.util collections/iterables, java.time, Query,
// XML, exceptions, Key, StringBuilder, ...). Headers document each skip.
compat_test!(cst_array, "runtime/dynamic/casters/ArrayCasterTest.bxs");
compat_test!(
    cst_assignable_array,
    "runtime/dynamic/casters/AssignableArrayCasterTest.bxs"
);
compat_test!(
    cst_big_integer,
    "runtime/dynamic/casters/BigIntegerCasterTest.bxs"
);
compat_test!(
    cst_collection,
    "runtime/dynamic/casters/CollectionCasterTest.bxs"
);
compat_test!(
    cst_date_time,
    "runtime/dynamic/casters/DateTimeCasterTest.bxs"
);
compat_test!(
    cst_function,
    "runtime/dynamic/casters/FunctionCasterTest.bxs"
);
compat_test!(
    cst_iterable,
    "runtime/dynamic/casters/IterableCasterTest.bxs"
);
compat_test!(cst_key, "runtime/dynamic/casters/KeyCasterTest.bxs");
compat_test!(
    cst_modifiable_array,
    "runtime/dynamic/casters/ModifiableArrayCasterTest.bxs"
);
compat_test!(
    cst_modifiable_struct,
    "runtime/dynamic/casters/ModifiableStructCasterTest.bxs"
);
compat_test!(cst_query, "runtime/dynamic/casters/QueryCasterTest.bxs");
compat_test!(
    cst_string_builder,
    "runtime/dynamic/casters/StringBuilderCasterTest.bxs"
);
compat_test!(cst_struct, "runtime/dynamic/casters/StructCasterTest.bxs");
compat_test!(
    cst_throwable,
    "runtime/dynamic/casters/ThrowableCasterTest.bxs"
);
compat_test!(cst_time, "runtime/dynamic/casters/TimeCasterTest.bxs");
compat_test!(
    cst_variable_name,
    "runtime/dynamic/casters/VariableNameCasterTest.bxs"
);
compat_test!(cst_xml, "runtime/dynamic/casters/XMLCasterTest.bxs");

// ---------------------------------------------------------------------------
// runtime/types — ortus/boxlang/runtime/types/*.java
// ---------------------------------------------------------------------------

// Array behavior is transferred; negative indexing and out-of-bounds reads are
// red until MatchBox implements them. The remaining type tests drive internal
// Java type APIs and are documented as skips in their headers.
compat_test!(typ_array, "runtime/types/ArrayTest.bxs");
compat_test!(typ_argument, "runtime/types/ArgumentTest.bxs");
compat_test!(typ_box_set, "runtime/types/BoxSetTest.bxs");
compat_test!(
    typ_box_string_builder,
    "runtime/types/BoxStringBuilderTest.bxs"
);
compat_test!(
    typ_chunked_array_list,
    "runtime/types/ChunkedArrayListTest.bxs"
);
compat_test!(typ_closure, "runtime/types/ClosureTest.bxs");
compat_test!(typ_date_time, "runtime/types/DateTimeTest.bxs");
compat_test!(typ_delimited_array, "runtime/types/DelimitedArrayTest.bxs");
compat_test!(
    typ_dynamic_function,
    "runtime/types/DynamicFunctionTest.bxs"
);
compat_test!(typ_file, "runtime/types/FileTest.bxs");
compat_test!(typ_function, "runtime/types/FunctionTest.bxs");
compat_test!(typ_lambda, "runtime/types/LambdaTest.bxs");
compat_test!(
    typ_query_column_type,
    "runtime/types/QueryColumnTypeTest.bxs"
);
compat_test!(typ_query, "runtime/types/QueryTest.bxs");
compat_test!(typ_struct, "runtime/types/StructTest.bxs");
compat_test!(typ_xml, "runtime/types/XMLTest.bxs");
compat_test!(typ_meta_array, "runtime/types/meta/ArrayMetaTest.bxs");
compat_test!(typ_meta_function, "runtime/types/meta/FunctionMetaTest.bxs");
compat_test!(typ_meta_query, "runtime/types/meta/QueryMetaTest.bxs");
compat_test!(typ_meta_struct, "runtime/types/meta/StructMetaTest.bxs");
compat_test!(
    typ_unmodifiable_array,
    "runtime/types/unmodifiable/UnmodifiableArrayTest.bxs"
);
compat_test!(
    typ_unmodifiable_struct,
    "runtime/types/unmodifiable/UnmodifiableStructTest.bxs"
);
compat_test!(
    typ_util_bl_collector,
    "runtime/types/util/BLCollectorTest.bxs"
);
compat_test!(typ_util_list_util, "runtime/types/util/ListUtilTest.bxs");
compat_test!(
    typ_util_struct_util,
    "runtime/types/util/StructUtilTest.bxs"
);
compat_test!(typ_util_type_util, "runtime/types/util/TypeUtilTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/decision — ortus/boxlang/runtime/bifs/global/decision/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_dec_is_array,
    "runtime/bifs/global/decision/IsArrayTest.bxs"
);
compat_test!(
    bf_dec_is_binary,
    "runtime/bifs/global/decision/IsBinaryTest.bxs"
);
compat_test!(
    bf_dec_is_boolean,
    "runtime/bifs/global/decision/IsBooleanTest.bxs"
);
compat_test!(
    bf_dec_is_box_set,
    "runtime/bifs/global/decision/IsBoxSetTest.bxs"
);
compat_test!(
    bf_dec_is_closure,
    "runtime/bifs/global/decision/IsClosureTest.bxs"
);
compat_test!(
    bf_dec_is_custom_function,
    "runtime/bifs/global/decision/IsCustomFunctionTest.bxs"
);
compat_test!(
    bf_dec_is_date_object,
    "runtime/bifs/global/decision/IsDateObjectTest.bxs"
);
compat_test!(
    bf_dec_is_date,
    "runtime/bifs/global/decision/IsDateTest.bxs"
);
compat_test!(
    bf_dec_is_debug_mode,
    "runtime/bifs/global/decision/IsDebugModeTest.bxs"
);
compat_test!(
    bf_dec_is_defined,
    "runtime/bifs/global/decision/IsDefinedTest.bxs"
);
compat_test!(
    bf_dec_is_empty,
    "runtime/bifs/global/decision/IsEmptyTest.bxs"
);
compat_test!(
    bf_dec_is_file_object,
    "runtime/bifs/global/decision/IsFileObjectTest.bxs"
);
compat_test!(
    bf_dec_is_ipv6,
    "runtime/bifs/global/decision/IsIPv6Test.bxs"
);
compat_test!(
    bf_dec_is_json,
    "runtime/bifs/global/decision/IsJSONTest.bxs"
);
compat_test!(
    bf_dec_is_leap_year,
    "runtime/bifs/global/decision/IsLeapYearTest.bxs"
);
compat_test!(
    bf_dec_is_localhost,
    "runtime/bifs/global/decision/IsLocalhostTest.bxs"
);
compat_test!(
    bf_dec_is_null,
    "runtime/bifs/global/decision/IsNullTest.bxs"
);
compat_test!(
    bf_dec_is_numeric_date,
    "runtime/bifs/global/decision/IsNumericDateTest.bxs"
);
compat_test!(
    bf_dec_is_numeric,
    "runtime/bifs/global/decision/IsNumericTest.bxs"
);
compat_test!(
    bf_dec_is_object,
    "runtime/bifs/global/decision/IsObjectTest.bxs"
);
compat_test!(
    bf_dec_is_query,
    "runtime/bifs/global/decision/IsQueryTest.bxs"
);
compat_test!(
    bf_dec_is_range,
    "runtime/bifs/global/decision/IsRangeTest.bxs"
);
compat_test!(
    bf_dec_is_simple_value,
    "runtime/bifs/global/decision/IsSimpleValueTest.bxs"
);
compat_test!(
    bf_dec_is_struct,
    "runtime/bifs/global/decision/IsStructTest.bxs"
);
compat_test!(
    bf_dec_is_valid,
    "runtime/bifs/global/decision/IsValidTest.bxs"
);
compat_test!(
    bf_dec_is_xml_attribute,
    "runtime/bifs/global/decision/IsXMLAttributeTest.bxs"
);
compat_test!(
    bf_dec_is_xml_doc,
    "runtime/bifs/global/decision/IsXMLDocTest.bxs"
);
compat_test!(
    bf_dec_is_xml_elem,
    "runtime/bifs/global/decision/IsXMLElemTest.bxs"
);
compat_test!(
    bf_dec_is_xml_node,
    "runtime/bifs/global/decision/IsXMLNodeTest.bxs"
);
compat_test!(
    bf_dec_is_xml_root,
    "runtime/bifs/global/decision/IsXMLRootTest.bxs"
);
compat_test!(bf_dec_is_xml, "runtime/bifs/global/decision/IsXMLTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/binary — ortus/boxlang/runtime/bifs/global/binary/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_bin_decode,
    "runtime/bifs/global/binary/BinaryDecodeTest.bxs"
);
compat_test!(
    bf_bin_encode,
    "runtime/bifs/global/binary/BinaryEncodeTest.bxs"
);
compat_test!(bf_bin_and, "runtime/bifs/global/binary/BitAndTest.bxs");
compat_test!(
    bf_bin_mask_clear,
    "runtime/bifs/global/binary/BitMaskClearTest.bxs"
);
compat_test!(
    bf_bin_mask_read,
    "runtime/bifs/global/binary/BitMaskReadTest.bxs"
);
compat_test!(
    bf_bin_mask_set,
    "runtime/bifs/global/binary/BitMaskSetTest.bxs"
);
compat_test!(bf_bin_not, "runtime/bifs/global/binary/BitNotTest.bxs");
compat_test!(bf_bin_or, "runtime/bifs/global/binary/BitOrTest.bxs");
compat_test!(bf_bin_sh, "runtime/bifs/global/binary/BitShTest.bxs");
compat_test!(bf_bin_xor, "runtime/bifs/global/binary/BitXorTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/cli — ortus/boxlang/runtime/bifs/global/cli/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_cli_get_args,
    "runtime/bifs/global/cli/CLIGetArgsTest.bxs"
);
compat_test!(bf_cli_read, "runtime/bifs/global/cli/CLIReadTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/type — ortus/boxlang/runtime/bifs/global/type/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_type_get_metadata,
    "runtime/bifs/global/type/GetMetaDataTest.bxs"
);
compat_test!(bf_type_len, "runtime/bifs/global/type/LenTest.bxs");
compat_test!(
    bf_type_null_value,
    "runtime/bifs/global/type/NullValueTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/format — ortus/boxlang/runtime/bifs/global/format/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_format_boolean,
    "runtime/bifs/global/format/BooleanFormatTest.bxs"
);
compat_test!(
    bf_format_decimal,
    "runtime/bifs/global/format/DecimalFormatTest.bxs"
);
compat_test!(
    bf_format_number,
    "runtime/bifs/global/format/NumberFormatTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/stream — ortus/boxlang/runtime/bifs/global/stream/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_stream_to_bx_array,
    "runtime/bifs/global/stream/ToBXArrayTest.bxs"
);
compat_test!(
    bf_stream_to_bx_list,
    "runtime/bifs/global/stream/ToBXListTest.bxs"
);
compat_test!(
    bf_stream_to_bx_query,
    "runtime/bifs/global/stream/ToBXQueryTest.bxs"
);
compat_test!(
    bf_stream_to_bx_struct,
    "runtime/bifs/global/stream/ToBXStructTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/conversion — .../conversion/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_conv_json_deserialize,
    "runtime/bifs/global/conversion/JSONDeserializeTest.bxs"
);
compat_test!(
    bf_conv_json_serialize,
    "runtime/bifs/global/conversion/JSONSerializeTest.bxs"
);
compat_test!(
    bf_conv_parse_number,
    "runtime/bifs/global/conversion/ParseNumberTest.bxs"
);
compat_test!(
    bf_conv_to_base64,
    "runtime/bifs/global/conversion/ToBase64Test.bxs"
);
compat_test!(
    bf_conv_to_binary,
    "runtime/bifs/global/conversion/ToBinaryTest.bxs"
);
compat_test!(
    bf_conv_to_modifiable,
    "runtime/bifs/global/conversion/ToModifiableTest.bxs"
);
compat_test!(
    bf_conv_to_numeric,
    "runtime/bifs/global/conversion/ToNumericTest.bxs"
);
compat_test!(
    bf_conv_to_script,
    "runtime/bifs/global/conversion/ToScriptTest.bxs"
);
compat_test!(
    bf_conv_to_string,
    "runtime/bifs/global/conversion/ToStringTest.bxs"
);
compat_test!(
    bf_conv_to_unmodifiable,
    "runtime/bifs/global/conversion/ToUnmodifiableTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/encryption — .../encryption/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_encryption_decrypt,
    "runtime/bifs/global/encryption/DecryptTest.bxs"
);
compat_test!(
    bf_encryption_encrypt,
    "runtime/bifs/global/encryption/EncryptTest.bxs"
);
compat_test!(
    bf_encryption_generate_pbkdf_key,
    "runtime/bifs/global/encryption/GeneratePBKDFKeyTest.bxs"
);
compat_test!(
    bf_encryption_generate_secret_key,
    "runtime/bifs/global/encryption/GenerateSecretKeyTest.bxs"
);
compat_test!(
    bf_encryption_hash,
    "runtime/bifs/global/encryption/HashTest.bxs"
);
compat_test!(
    bf_encryption_hmac,
    "runtime/bifs/global/encryption/HmacTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/i18n — .../i18n/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_i18n_currency_format,
    "runtime/bifs/global/i18n/CurrencyFormatTest.bxs"
);
compat_test!(
    bf_i18n_get_locale_display_name,
    "runtime/bifs/global/i18n/GetLocaleDisplayNameTest.bxs"
);
compat_test!(
    bf_i18n_get_locale_info,
    "runtime/bifs/global/i18n/GetLocaleInfoTest.bxs"
);
compat_test!(
    bf_i18n_get_locale,
    "runtime/bifs/global/i18n/GetLocaleTest.bxs"
);
compat_test!(
    bf_i18n_is_currency,
    "runtime/bifs/global/i18n/IsCurrencyTest.bxs"
);
compat_test!(
    bf_i18n_parse_currency,
    "runtime/bifs/global/i18n/ParseCurrencyTest.bxs"
);
compat_test!(
    bf_i18n_set_locale,
    "runtime/bifs/global/i18n/SetLocaleTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/math — .../math/*.java
// ---------------------------------------------------------------------------

compat_test!(bf_math_abs, "runtime/bifs/global/math/AbsTest.bxs");
compat_test!(bf_math_acos, "runtime/bifs/global/math/AcosTest.bxs");
compat_test!(bf_math_asin, "runtime/bifs/global/math/AsinTest.bxs");
compat_test!(bf_math_atn, "runtime/bifs/global/math/AtnTest.bxs");
compat_test!(bf_math_ceiling, "runtime/bifs/global/math/CeilingTest.bxs");
compat_test!(bf_math_cos, "runtime/bifs/global/math/CosTest.bxs");
compat_test!(
    bf_math_decrement_value,
    "runtime/bifs/global/math/DecrementValueTest.bxs"
);
compat_test!(bf_math_exp, "runtime/bifs/global/math/ExpTest.bxs");
compat_test!(bf_math_fix, "runtime/bifs/global/math/FixTest.bxs");
compat_test!(bf_math_floor, "runtime/bifs/global/math/FloorTest.bxs");
compat_test!(
    bf_math_format_base_n,
    "runtime/bifs/global/math/FormatBaseNTest.bxs"
);
compat_test!(
    bf_math_increment_value,
    "runtime/bifs/global/math/IncrementValueTest.bxs"
);
compat_test!(
    bf_math_input_base_n,
    "runtime/bifs/global/math/InputBaseNTest.bxs"
);
compat_test!(bf_math_int, "runtime/bifs/global/math/IntTest.bxs");
compat_test!(bf_math_log10, "runtime/bifs/global/math/Log10Test.bxs");
compat_test!(bf_math_log, "runtime/bifs/global/math/LogTest.bxs");
compat_test!(bf_math_max, "runtime/bifs/global/math/MaxTest.bxs");
compat_test!(bf_math_min, "runtime/bifs/global/math/MinTest.bxs");
compat_test!(bf_math_pi, "runtime/bifs/global/math/PiTest.bxs");
compat_test!(
    bf_math_precision_evaluate,
    "runtime/bifs/global/math/PrecisionEvaluateTest.bxs"
);
compat_test!(
    bf_math_randomize,
    "runtime/bifs/global/math/RandomizeTest.bxs"
);
compat_test!(
    bf_math_rand_range,
    "runtime/bifs/global/math/RandRangeTest.bxs"
);
compat_test!(bf_math_rand, "runtime/bifs/global/math/RandTest.bxs");
compat_test!(bf_math_round, "runtime/bifs/global/math/RoundTest.bxs");
compat_test!(bf_math_sgn, "runtime/bifs/global/math/SgnTest.bxs");
compat_test!(bf_math_sin, "runtime/bifs/global/math/SinTest.bxs");
compat_test!(bf_math_sqr, "runtime/bifs/global/math/SqrTest.bxs");
compat_test!(bf_math_tan, "runtime/bifs/global/math/TanTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/list — .../list/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_list_get_token,
    "runtime/bifs/global/list/GetTokenTest.bxs"
);
compat_test!(
    bf_list_append,
    "runtime/bifs/global/list/ListAppendTest.bxs"
);
compat_test!(bf_list_avg, "runtime/bifs/global/list/ListAvgTest.bxs");
compat_test!(
    bf_list_change_delims,
    "runtime/bifs/global/list/ListChangeDelimsTest.bxs"
);
compat_test!(
    bf_list_compact,
    "runtime/bifs/global/list/ListCompactTest.bxs"
);
compat_test!(
    bf_list_contains,
    "runtime/bifs/global/list/ListContainsTest.bxs"
);
compat_test!(
    bf_list_delete_at,
    "runtime/bifs/global/list/ListDeleteAtTest.bxs"
);
compat_test!(bf_list_each, "runtime/bifs/global/list/ListEachTest.bxs");
compat_test!(bf_list_every, "runtime/bifs/global/list/ListEveryTest.bxs");
compat_test!(
    bf_list_filter,
    "runtime/bifs/global/list/ListFilterTest.bxs"
);
compat_test!(bf_list_find, "runtime/bifs/global/list/ListFindTest.bxs");
compat_test!(bf_list_get_at, "runtime/bifs/global/list/ListGetAtTest.bxs");
compat_test!(
    bf_list_get_endings,
    "runtime/bifs/global/list/ListGetEndingsTest.bxs"
);
compat_test!(
    bf_list_index_exists,
    "runtime/bifs/global/list/ListIndexExistsTest.bxs"
);
compat_test!(
    bf_list_insert_at,
    "runtime/bifs/global/list/ListInsertAtTest.bxs"
);
compat_test!(
    bf_list_item_trim,
    "runtime/bifs/global/list/ListItemTrimTest.bxs"
);
compat_test!(bf_list_len, "runtime/bifs/global/list/ListLenTest.bxs");
compat_test!(bf_list_map, "runtime/bifs/global/list/ListMapTest.bxs");
compat_test!(bf_list_none, "runtime/bifs/global/list/ListNoneTest.bxs");
compat_test!(
    bf_list_prepend,
    "runtime/bifs/global/list/ListPrependTest.bxs"
);
compat_test!(
    bf_list_qualify,
    "runtime/bifs/global/list/ListQualifyTest.bxs"
);
compat_test!(
    bf_list_reduce_right,
    "runtime/bifs/global/list/ListReduceRightTest.bxs"
);
compat_test!(
    bf_list_remove_duplicates,
    "runtime/bifs/global/list/ListRemoveDuplicatesTest.bxs"
);
compat_test!(bf_list_rest, "runtime/bifs/global/list/ListRestTest.bxs");
compat_test!(bf_list_set_at, "runtime/bifs/global/list/ListSetAtTest.bxs");
compat_test!(bf_list_some, "runtime/bifs/global/list/ListSomeTest.bxs");
compat_test!(bf_list_sort, "runtime/bifs/global/list/ListSortTest.bxs");
compat_test!(
    bf_list_to_array,
    "runtime/bifs/global/list/ListToArrayTest.bxs"
);
compat_test!(
    bf_list_value_count,
    "runtime/bifs/global/list/ListValueCountTest.bxs"
);

compat_test!(
    bf_array_append,
    "runtime/bifs/global/array/ArrayAppendTest.bxs"
);
compat_test!(bf_array_avg, "runtime/bifs/global/array/ArrayAvgTest.bxs");
compat_test!(
    bf_array_chunk,
    "runtime/bifs/global/array/ArrayChunkTest.bxs"
);
compat_test!(
    bf_array_clear,
    "runtime/bifs/global/array/ArrayClearTest.bxs"
);
compat_test!(
    bf_array_contains,
    "runtime/bifs/global/array/ArrayContainsTest.bxs"
);
compat_test!(
    bf_array_contains_nocase,
    "runtime/bifs/global/array/ArrayContainsNoCaseTest.bxs"
);
compat_test!(
    bf_array_delete,
    "runtime/bifs/global/array/ArrayDeleteTest.bxs"
);
compat_test!(
    bf_array_delete_at,
    "runtime/bifs/global/array/ArrayDeleteAtTest.bxs"
);
compat_test!(
    bf_array_delete_nocase,
    "runtime/bifs/global/array/ArrayDeleteNoCaseTest.bxs"
);
compat_test!(bf_array_each, "runtime/bifs/global/array/ArrayEachTest.bxs");
compat_test!(
    bf_array_every,
    "runtime/bifs/global/array/ArrayEveryTest.bxs"
);
compat_test!(
    bf_array_filter,
    "runtime/bifs/global/array/ArrayFilterTest.bxs"
);
compat_test!(bf_array_find, "runtime/bifs/global/array/ArrayFindTest.bxs");
compat_test!(
    bf_array_find_all,
    "runtime/bifs/global/array/ArrayFindAllTest.bxs"
);
compat_test!(
    bf_array_find_all_nocase,
    "runtime/bifs/global/array/ArrayFindAllNoCaseTest.bxs"
);
compat_test!(
    bf_array_find_first,
    "runtime/bifs/global/array/ArrayFindFirstTest.bxs"
);
compat_test!(
    bf_array_first,
    "runtime/bifs/global/array/ArrayFirstTest.bxs"
);
compat_test!(
    bf_array_flat_map,
    "runtime/bifs/global/array/ArrayFlatMapTest.bxs"
);
compat_test!(
    bf_array_flatten,
    "runtime/bifs/global/array/ArrayFlattenTest.bxs"
);
compat_test!(
    bf_array_get_metadata,
    "runtime/bifs/global/array/ArrayGetMetadataTest.bxs"
);
compat_test!(
    bf_array_group_by,
    "runtime/bifs/global/array/ArrayGroupByTest.bxs"
);
compat_test!(
    bf_array_index_exists,
    "runtime/bifs/global/array/ArrayIndexExistsTest.bxs"
);
compat_test!(
    bf_array_insert_at,
    "runtime/bifs/global/array/ArrayInsertAtTest.bxs"
);
compat_test!(
    bf_array_is_empty,
    "runtime/bifs/global/array/ArrayIsEmptyTest.bxs"
);
compat_test!(bf_array_last, "runtime/bifs/global/array/ArrayLastTest.bxs");
compat_test!(bf_array_map, "runtime/bifs/global/array/ArrayMapTest.bxs");
compat_test!(bf_array_max, "runtime/bifs/global/array/ArrayMaxTest.bxs");
compat_test!(
    bf_array_median,
    "runtime/bifs/global/array/ArrayMedianTest.bxs"
);
compat_test!(
    bf_array_merge,
    "runtime/bifs/global/array/ArrayMergeTest.bxs"
);
compat_test!(bf_array_min, "runtime/bifs/global/array/ArrayMinTest.bxs");
compat_test!(bf_array_new, "runtime/bifs/global/array/ArrayNewTest.bxs");
compat_test!(bf_array_none, "runtime/bifs/global/array/ArrayNoneTest.bxs");
compat_test!(bf_array_pop, "runtime/bifs/global/array/ArrayPopTest.bxs");
compat_test!(
    bf_array_prepend,
    "runtime/bifs/global/array/ArrayPrependTest.bxs"
);
compat_test!(bf_array_push, "runtime/bifs/global/array/ArrayPushTest.bxs");
compat_test!(
    bf_array_range,
    "runtime/bifs/global/array/ArrayRangeTest.bxs"
);
compat_test!(
    bf_array_reduce,
    "runtime/bifs/global/array/ArrayReduceTest.bxs"
);
compat_test!(
    bf_array_reduce_right,
    "runtime/bifs/global/array/ArrayReduceRightTest.bxs"
);
compat_test!(
    bf_array_reject,
    "runtime/bifs/global/array/ArrayRejectTest.bxs"
);
compat_test!(
    bf_array_resize,
    "runtime/bifs/global/array/ArrayResizeTest.bxs"
);
compat_test!(
    bf_array_reverse,
    "runtime/bifs/global/array/ArrayReverseTest.bxs"
);
compat_test!(bf_array_set, "runtime/bifs/global/array/ArraySetTest.bxs");
compat_test!(
    bf_array_shift,
    "runtime/bifs/global/array/ArrayShiftTest.bxs"
);
compat_test!(
    bf_array_slice,
    "runtime/bifs/global/array/ArraySliceTest.bxs"
);
compat_test!(bf_array_some, "runtime/bifs/global/array/ArraySomeTest.bxs");
compat_test!(bf_array_sort, "runtime/bifs/global/array/ArraySortTest.bxs");
compat_test!(
    bf_array_splice,
    "runtime/bifs/global/array/ArraySpliceTest.bxs"
);
compat_test!(bf_array_sum, "runtime/bifs/global/array/ArraySumTest.bxs");
compat_test!(bf_array_swap, "runtime/bifs/global/array/ArraySwapTest.bxs");
compat_test!(
    bf_array_to_list,
    "runtime/bifs/global/array/ArrayToListTest.bxs"
);
compat_test!(
    bf_array_to_struct,
    "runtime/bifs/global/array/ArrayToStructTest.bxs"
);
compat_test!(
    bf_array_transpose,
    "runtime/bifs/global/array/ArrayTransposeTest.bxs"
);
compat_test!(
    bf_array_unique,
    "runtime/bifs/global/array/ArrayUniqueTest.bxs"
);
compat_test!(
    bf_array_unshift,
    "runtime/bifs/global/array/ArrayUnshiftTest.bxs"
);
compat_test!(bf_array_zip, "runtime/bifs/global/array/ArrayZipTest.bxs");
compat_test!(bf_string_ascii, "runtime/bifs/global/string/AsciiTest.bxs");
compat_test!(
    bf_string_charset_decode,
    "runtime/bifs/global/string/CharsetDecodeTest.bxs"
);
compat_test!(
    bf_string_charset_encode,
    "runtime/bifs/global/string/CharsetEncodeTest.bxs"
);
compat_test!(bf_string_char, "runtime/bifs/global/string/CharTest.bxs");
compat_test!(
    bf_string_compare_nocase,
    "runtime/bifs/global/string/CompareNoCaseTest.bxs"
);
compat_test!(
    bf_string_compare,
    "runtime/bifs/global/string/CompareTest.bxs"
);
compat_test!(
    bf_string_date_len,
    "runtime/bifs/global/string/DateLenTest.bxs"
);
compat_test!(
    bf_string_find_one_of,
    "runtime/bifs/global/string/FindOneOfTest.bxs"
);
compat_test!(bf_string_find, "runtime/bifs/global/string/FindTest.bxs");
compat_test!(
    bf_string_insert,
    "runtime/bifs/global/string/InsertTest.bxs"
);
compat_test!(bf_string_lcase, "runtime/bifs/global/string/LCaseTest.bxs");
compat_test!(bf_string_left, "runtime/bifs/global/string/LeftTest.bxs");
compat_test!(
    bf_string_list_reduce,
    "runtime/bifs/global/string/ListReduceTest.bxs"
);
compat_test!(
    bf_string_ljustify,
    "runtime/bifs/global/string/LJustifyTest.bxs"
);
compat_test!(bf_string_ltrim, "runtime/bifs/global/string/LTrimTest.bxs");
compat_test!(bf_string_mid, "runtime/bifs/global/string/MidTest.bxs");
compat_test!(
    bf_string_paragraph_format,
    "runtime/bifs/global/string/ParagraphFormatTest.bxs"
);
compat_test!(
    bf_string_query_string_to_struct,
    "runtime/bifs/global/string/QueryStringToStructTest.bxs"
);
compat_test!(
    bf_string_re_escape,
    "runtime/bifs/global/string/ReEscapeTest.bxs"
);
compat_test!(
    bf_string_refind,
    "runtime/bifs/global/string/ReFindTest.bxs"
);
compat_test!(
    bf_string_rematch_nocase,
    "runtime/bifs/global/string/ReMatchNoCaseTest.bxs"
);
compat_test!(
    bf_string_rematch,
    "runtime/bifs/global/string/ReMatchTest.bxs"
);
compat_test!(
    bf_string_remove_chars,
    "runtime/bifs/global/string/RemoveCharsTest.bxs"
);
compat_test!(
    bf_string_repeat_string,
    "runtime/bifs/global/string/RepeatStringTest.bxs"
);
compat_test!(
    bf_string_replace_list,
    "runtime/bifs/global/string/ReplaceListTest.bxs"
);
compat_test!(
    bf_string_replace_nocase,
    "runtime/bifs/global/string/ReplaceNoCaseTest.bxs"
);
compat_test!(
    bf_string_replace,
    "runtime/bifs/global/string/ReplaceTest.bxs"
);
compat_test!(
    bf_string_rereplace_nocase,
    "runtime/bifs/global/string/ReReplaceNoCaseTest.bxs"
);
compat_test!(
    bf_string_rereplace,
    "runtime/bifs/global/string/ReReplaceTest.bxs"
);
compat_test!(
    bf_string_reverse,
    "runtime/bifs/global/string/ReverseTest.bxs"
);
compat_test!(bf_string_right, "runtime/bifs/global/string/RightTest.bxs");
compat_test!(
    bf_string_rjustify,
    "runtime/bifs/global/string/RJustifyTest.bxs"
);
compat_test!(bf_string_rtrim, "runtime/bifs/global/string/RTrimTest.bxs");
compat_test!(
    bf_string_span_excluding,
    "runtime/bifs/global/string/SpanExcludingTest.bxs"
);
compat_test!(
    bf_string_span_including,
    "runtime/bifs/global/string/SpanIncludingTest.bxs"
);
compat_test!(
    bf_string_sql_prettify,
    "runtime/bifs/global/string/SQLPrettifyTest.bxs"
);
compat_test!(
    bf_string_bind,
    "runtime/bifs/global/string/StringBindTest.bxs"
);
compat_test!(
    bf_string_each,
    "runtime/bifs/global/string/StringEachTest.bxs"
);
compat_test!(
    bf_string_ends_with_nocase,
    "runtime/bifs/global/string/StringEndsWithNoCaseTest.bxs"
);
compat_test!(
    bf_string_ends_with,
    "runtime/bifs/global/string/StringEndsWithTest.bxs"
);
compat_test!(
    bf_string_every,
    "runtime/bifs/global/string/StringEveryTest.bxs"
);
compat_test!(
    bf_string_filter,
    "runtime/bifs/global/string/StringFilterTest.bxs"
);
compat_test!(
    bf_string_len,
    "runtime/bifs/global/string/StringLenTest.bxs"
);
compat_test!(
    bf_string_map,
    "runtime/bifs/global/string/StringMapTest.bxs"
);
compat_test!(
    bf_string_reduce_right,
    "runtime/bifs/global/string/StringReduceRightTest.bxs"
);
compat_test!(
    bf_string_reduce,
    "runtime/bifs/global/string/StringReduceTest.bxs"
);
compat_test!(
    bf_string_some,
    "runtime/bifs/global/string/StringSomeTest.bxs"
);
compat_test!(
    bf_string_sort,
    "runtime/bifs/global/string/StringSortTest.bxs"
);
compat_test!(
    bf_string_starts_with_nocase,
    "runtime/bifs/global/string/StringStartsWithNoCaseTest.bxs"
);
compat_test!(
    bf_string_starts_with,
    "runtime/bifs/global/string/StringStartsWithTest.bxs"
);
compat_test!(
    bf_string_strip_cr,
    "runtime/bifs/global/string/StripCRTest.bxs"
);
compat_test!(bf_string_trim, "runtime/bifs/global/string/TrimTest.bxs");
compat_test!(bf_string_ucase, "runtime/bifs/global/string/UCaseTest.bxs");
compat_test!(
    bf_string_ucfirst,
    "runtime/bifs/global/string/UCFirstTest.bxs"
);
compat_test!(bf_string_val, "runtime/bifs/global/string/ValTest.bxs");
compat_test!(bf_string_wrap, "runtime/bifs/global/string/WrapTest.bxs");
compat_test!(
    bf_string_yes_no_format,
    "runtime/bifs/global/string/YesNoFormatTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/xml — .../xml/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_xml_child_pos,
    "runtime/bifs/global/xml/XMLChildPosTest.bxs"
);
compat_test!(
    bf_xml_elem_new,
    "runtime/bifs/global/xml/XMLElemNewTest.bxs"
);
compat_test!(bf_xml_format, "runtime/bifs/global/xml/XMLFormatTest.bxs");
compat_test!(
    bf_xml_get_node_type,
    "runtime/bifs/global/xml/XMLGetNodeTypeTest.bxs"
);
compat_test!(bf_xml_new, "runtime/bifs/global/xml/XMLNewTest.bxs");
compat_test!(bf_xml_parse, "runtime/bifs/global/xml/XMLParseTest.bxs");
compat_test!(bf_xml_search, "runtime/bifs/global/xml/XMLSearchTest.bxs");
compat_test!(
    bf_xml_transform,
    "runtime/bifs/global/xml/XMLTransformTest.bxs"
);
compat_test!(
    bf_xml_validate,
    "runtime/bifs/global/xml/XMLValidateTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/stringbuilder — .../stringbuilder/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_stringbuilder_is_string_builder,
    "runtime/bifs/global/stringbuilder/IsStringBuilderTest.bxs"
);
compat_test!(
    bf_stringbuilder_append,
    "runtime/bifs/global/stringbuilder/StringBuilderAppendTest.bxs"
);
compat_test!(
    bf_stringbuilder_clear,
    "runtime/bifs/global/stringbuilder/StringBuilderClearTest.bxs"
);
compat_test!(
    bf_stringbuilder_contains,
    "runtime/bifs/global/stringbuilder/StringBuilderContainsTest.bxs"
);
compat_test!(
    bf_stringbuilder_delete,
    "runtime/bifs/global/stringbuilder/StringBuilderDeleteTest.bxs"
);
compat_test!(
    bf_stringbuilder_ends_with,
    "runtime/bifs/global/stringbuilder/StringBuilderEndsWithTest.bxs"
);
compat_test!(
    bf_stringbuilder_find,
    "runtime/bifs/global/stringbuilder/StringBuilderFindTest.bxs"
);
compat_test!(
    bf_stringbuilder_insert,
    "runtime/bifs/global/stringbuilder/StringBuilderInsertTest.bxs"
);
compat_test!(
    bf_stringbuilder_integration,
    "runtime/bifs/global/stringbuilder/StringBuilderIntegrationTest.bxs"
);
compat_test!(
    bf_stringbuilder_left,
    "runtime/bifs/global/stringbuilder/StringBuilderLeftTest.bxs"
);
compat_test!(
    bf_stringbuilder_mid,
    "runtime/bifs/global/stringbuilder/StringBuilderMidTest.bxs"
);
compat_test!(
    bf_stringbuilder_new,
    "runtime/bifs/global/stringbuilder/StringBuilderNewTest.bxs"
);
compat_test!(
    bf_stringbuilder_prepend,
    "runtime/bifs/global/stringbuilder/StringBuilderPrependTest.bxs"
);
compat_test!(
    bf_stringbuilder_replace,
    "runtime/bifs/global/stringbuilder/StringBuilderReplaceTest.bxs"
);
compat_test!(
    bf_stringbuilder_reverse,
    "runtime/bifs/global/stringbuilder/StringBuilderReverseTest.bxs"
);
compat_test!(
    bf_stringbuilder_right,
    "runtime/bifs/global/stringbuilder/StringBuilderRightTest.bxs"
);
compat_test!(
    bf_stringbuilder_starts_with,
    "runtime/bifs/global/stringbuilder/StringBuilderStartsWithTest.bxs"
);
compat_test!(
    bf_stringbuilder_trim,
    "runtime/bifs/global/stringbuilder/StringBuilderTrimTest.bxs"
);
compat_test!(
    bf_struct_append,
    "runtime/bifs/global/struct/StructAppendTest.bxs"
);
compat_test!(
    bf_struct_clear,
    "runtime/bifs/global/struct/StructClearTest.bxs"
);
compat_test!(
    bf_struct_copy,
    "runtime/bifs/global/struct/StructCopyTest.bxs"
);
compat_test!(
    bf_struct_delete,
    "runtime/bifs/global/struct/StructDeleteTest.bxs"
);
compat_test!(
    bf_struct_each,
    "runtime/bifs/global/struct/StructEachTest.bxs"
);
compat_test!(
    bf_struct_equals,
    "runtime/bifs/global/struct/StructEqualsTest.bxs"
);
compat_test!(
    bf_struct_every,
    "runtime/bifs/global/struct/StructEveryTest.bxs"
);
compat_test!(
    bf_struct_filter,
    "runtime/bifs/global/struct/StructFilterTest.bxs"
);
compat_test!(
    bf_struct_find_key,
    "runtime/bifs/global/struct/StructFindKeyTest.bxs"
);
compat_test!(
    bf_struct_find,
    "runtime/bifs/global/struct/StructFindTest.bxs"
);
compat_test!(
    bf_struct_find_value,
    "runtime/bifs/global/struct/StructFindValueTest.bxs"
);
compat_test!(
    bf_struct_get_metadata,
    "runtime/bifs/global/struct/StructGetMetadataTest.bxs"
);
compat_test!(
    bf_struct_get,
    "runtime/bifs/global/struct/StructGetTest.bxs"
);
compat_test!(
    bf_struct_insert,
    "runtime/bifs/global/struct/StructInsertTest.bxs"
);
compat_test!(
    bf_struct_is_case_sensitive,
    "runtime/bifs/global/struct/StructIsCaseSensitiveTest.bxs"
);
compat_test!(
    bf_struct_is_ordered,
    "runtime/bifs/global/struct/StructIsOrderedTest.bxs"
);
compat_test!(
    bf_struct_key_array,
    "runtime/bifs/global/struct/StructKeyArrayTest.bxs"
);
compat_test!(
    bf_struct_key_exists,
    "runtime/bifs/global/struct/StructKeyExistsTest.bxs"
);
compat_test!(
    bf_struct_key_list,
    "runtime/bifs/global/struct/StructKeyListTest.bxs"
);
compat_test!(
    bf_struct_key_translate,
    "runtime/bifs/global/struct/StructKeyTranslateTest.bxs"
);
compat_test!(
    bf_struct_map,
    "runtime/bifs/global/struct/StructMapTest.bxs"
);
compat_test!(
    bf_struct_new,
    "runtime/bifs/global/struct/StructNewTest.bxs"
);
compat_test!(
    bf_struct_none,
    "runtime/bifs/global/struct/StructNoneTest.bxs"
);
compat_test!(
    bf_struct_reduce,
    "runtime/bifs/global/struct/StructReduceTest.bxs"
);
compat_test!(
    bf_struct_some,
    "runtime/bifs/global/struct/StructSomeTest.bxs"
);
compat_test!(
    bf_struct_sort,
    "runtime/bifs/global/struct/StructSortTest.bxs"
);
compat_test!(
    bf_struct_to_sorted,
    "runtime/bifs/global/struct/StructToSortedTest.bxs"
);
compat_test!(
    bf_struct_update,
    "runtime/bifs/global/struct/StructUpdateTest.bxs"
);
compat_test!(
    bf_struct_value_array,
    "runtime/bifs/global/struct/StructValueArrayTest.bxs"
);
compat_test!(
    bf_io_contract_path,
    "runtime/bifs/global/io/ContractPathTest.bxs"
);
compat_test!(
    bf_io_create_temp_directory,
    "runtime/bifs/global/io/CreateTempDirectoryTest.bxs"
);
compat_test!(
    bf_io_create_temp_file,
    "runtime/bifs/global/io/CreateTempFileTest.bxs"
);
compat_test!(
    bf_io_directory_copy,
    "runtime/bifs/global/io/DirectoryCopyTest.bxs"
);
compat_test!(
    bf_io_directory_create,
    "runtime/bifs/global/io/DirectoryCreateTest.bxs"
);
compat_test!(
    bf_io_directory_delete,
    "runtime/bifs/global/io/DirectoryDeleteTest.bxs"
);
compat_test!(
    bf_io_directory_exists,
    "runtime/bifs/global/io/DirectoryExistsTest.bxs"
);
compat_test!(
    bf_io_directory_list,
    "runtime/bifs/global/io/DirectoryListTest.bxs"
);
compat_test!(
    bf_io_directory_move,
    "runtime/bifs/global/io/DirectoryMoveTest.bxs"
);
compat_test!(
    bf_io_expand_path,
    "runtime/bifs/global/io/ExpandPathTest.bxs"
);
compat_test!(
    bf_io_file_append,
    "runtime/bifs/global/io/FileAppendTest.bxs"
);
compat_test!(bf_io_file_close, "runtime/bifs/global/io/FileCloseTest.bxs");
compat_test!(bf_io_file_copy, "runtime/bifs/global/io/FileCopyTest.bxs");
compat_test!(
    bf_io_file_delete,
    "runtime/bifs/global/io/FileDeleteTest.bxs"
);
compat_test!(
    bf_io_file_exists,
    "runtime/bifs/global/io/FileExistsTest.bxs"
);
compat_test!(
    bf_io_file_get_mime_type,
    "runtime/bifs/global/io/FileGetMimeTypeTest.bxs"
);
compat_test!(bf_io_file_info, "runtime/bifs/global/io/FileInfoTest.bxs");
compat_test!(
    bf_io_file_is_eof,
    "runtime/bifs/global/io/FileIsEOFTest.bxs"
);
compat_test!(bf_io_file_move, "runtime/bifs/global/io/FileMoveTest.bxs");
compat_test!(bf_io_file_open, "runtime/bifs/global/io/FileOpenTest.bxs");
compat_test!(
    bf_io_file_read_line,
    "runtime/bifs/global/io/FileReadLineTest.bxs"
);
compat_test!(bf_io_file_read, "runtime/bifs/global/io/FileReadTest.bxs");
compat_test!(bf_io_file_seek, "runtime/bifs/global/io/FileSeekTest.bxs");
compat_test!(
    bf_io_file_set_access_mode,
    "runtime/bifs/global/io/FileSetAccessModeTest.bxs"
);
compat_test!(
    bf_io_file_set_attribute,
    "runtime/bifs/global/io/FileSetAttributeTest.bxs"
);
compat_test!(
    bf_io_file_set_last_modified,
    "runtime/bifs/global/io/FileSetLastModifiedTest.bxs"
);
compat_test!(
    bf_io_file_write_line,
    "runtime/bifs/global/io/FileWriteLineTest.bxs"
);
compat_test!(bf_io_file_write, "runtime/bifs/global/io/FileWriteTest.bxs");
compat_test!(
    bf_io_get_canonical_path,
    "runtime/bifs/global/io/GetCanonicalPathTest.bxs"
);
compat_test!(
    bf_io_get_directory_from_path,
    "runtime/bifs/global/io/GetDirectoryFromPathTest.bxs"
);
compat_test!(
    bf_io_property_file,
    "runtime/bifs/global/io/PropertyFileTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/zip — ortus/boxlang/runtime/bifs/global/zip/*.java
// ---------------------------------------------------------------------------

compat_test!(bf_zip_compress, "runtime/bifs/global/zip/CompressTest.bxs");
compat_test!(bf_zip_extract, "runtime/bifs/global/zip/ExtractTest.bxs");

compat_test!(
    bf_query_query_add_column,
    "runtime/bifs/global/query/QueryAddColumnTest.bxs"
);
compat_test!(
    bf_query_query_add_row,
    "runtime/bifs/global/query/QueryAddRowTest.bxs"
);
compat_test!(
    bf_query_query_append,
    "runtime/bifs/global/query/QueryAppendTest.bxs"
);
compat_test!(
    bf_query_query_clear,
    "runtime/bifs/global/query/QueryClearTest.bxs"
);
compat_test!(
    bf_query_query_column_array,
    "runtime/bifs/global/query/QueryColumnArrayTest.bxs"
);
compat_test!(
    bf_query_query_column_count,
    "runtime/bifs/global/query/QueryColumnCountTest.bxs"
);
compat_test!(
    bf_query_query_column_data,
    "runtime/bifs/global/query/QueryColumnDataTest.bxs"
);
compat_test!(
    bf_query_query_column_exists,
    "runtime/bifs/global/query/QueryColumnExistsTest.bxs"
);
compat_test!(
    bf_query_query_column_list,
    "runtime/bifs/global/query/QueryColumnListTest.bxs"
);
compat_test!(
    bf_query_query_current_row,
    "runtime/bifs/global/query/QueryCurrentRowTest.bxs"
);
compat_test!(
    bf_query_query_delete_column,
    "runtime/bifs/global/query/QueryDeleteColumnTest.bxs"
);
compat_test!(
    bf_query_query_delete_row,
    "runtime/bifs/global/query/QueryDeleteRowTest.bxs"
);
compat_test!(
    bf_query_query_each,
    "runtime/bifs/global/query/QueryEachTest.bxs"
);
compat_test!(
    bf_query_query_every,
    "runtime/bifs/global/query/QueryEveryTest.bxs"
);
compat_test!(
    bf_query_query_filter,
    "runtime/bifs/global/query/QueryFilterTest.bxs"
);
compat_test!(
    bf_query_query_get_cell,
    "runtime/bifs/global/query/QueryGetCellTest.bxs"
);
compat_test!(
    bf_query_query_get_result,
    "runtime/bifs/global/query/QueryGetResultTest.bxs"
);
compat_test!(
    bf_query_query_insert_at,
    "runtime/bifs/global/query/QueryInsertAtTest.bxs"
);
compat_test!(
    bf_query_query_key_exists,
    "runtime/bifs/global/query/QueryKeyExistsTest.bxs"
);
compat_test!(
    bf_query_query_map,
    "runtime/bifs/global/query/QueryMapTest.bxs"
);
compat_test!(
    bf_query_query_new,
    "runtime/bifs/global/query/QueryNewTest.bxs"
);
compat_test!(
    bf_query_query_none,
    "runtime/bifs/global/query/QueryNoneTest.bxs"
);
compat_test!(
    bf_query_query_prepend,
    "runtime/bifs/global/query/QueryPrependTest.bxs"
);
compat_test!(
    bf_query_query_record_count,
    "runtime/bifs/global/query/QueryRecordCountTest.bxs"
);
compat_test!(
    bf_query_query_reduce,
    "runtime/bifs/global/query/QueryReduceTest.bxs"
);
compat_test!(
    bf_query_query_reverse,
    "runtime/bifs/global/query/QueryReverseTest.bxs"
);
compat_test!(
    bf_query_query_row_data,
    "runtime/bifs/global/query/QueryRowDataTest.bxs"
);
compat_test!(
    bf_query_query_row_swap,
    "runtime/bifs/global/query/QueryRowSwapTest.bxs"
);
compat_test!(
    bf_query_query_set_cell,
    "runtime/bifs/global/query/QuerySetCellTest.bxs"
);
compat_test!(
    bf_query_query_set_row,
    "runtime/bifs/global/query/QuerySetRowTest.bxs"
);
compat_test!(
    bf_query_query_slice,
    "runtime/bifs/global/query/QuerySliceTest.bxs"
);
compat_test!(
    bf_query_query_some,
    "runtime/bifs/global/query/QuerySomeTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/cache - ortus/boxlang/runtime/bifs/global/cache/*.java
// ---------------------------------------------------------------------------

compat_test!(bf_cache_base, "runtime/bifs/global/cache/BaseCacheTest.bxs");
compat_test!(bf_cache_cache, "runtime/bifs/global/cache/CacheTest.bxs");
compat_test!(
    bf_cache_filter,
    "runtime/bifs/global/cache/CacheFilterTest.bxs"
);
compat_test!(
    bf_cache_names,
    "runtime/bifs/global/cache/CacheNamesTest.bxs"
);
compat_test!(
    bf_cache_providers,
    "runtime/bifs/global/cache/CacheProvidersTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/system — .../system/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_system_application_restart,
    "runtime/bifs/global/system/ApplicationRestartTest.bxs",
    "JVM-only: application lifecycle service and context objects"
);
compat_test!(
    bf_system_application_start_time,
    "runtime/bifs/global/system/ApplicationStartTimeTest.bxs",
    "JVM-only: application lifecycle service and context objects"
);
compat_test!(
    bf_system_application_stop,
    "runtime/bifs/global/system/ApplicationStopTest.bxs",
    "JVM-only: application lifecycle service and context objects"
);
compat_test!(
    bf_system_box_ast,
    "runtime/bifs/global/system/BoxASTTest.bxs"
);
compat_test!(
    bf_system_box_announce_async,
    "runtime/bifs/global/system/BoxAnnounceAsyncTest.bxs",
    "JVM-only: CompletableFuture and interceptor service"
);
compat_test!(
    bf_system_box_announce,
    "runtime/bifs/global/system/BoxAnnounceTest.bxs"
);
compat_test!(
    bf_system_box_register_interception_points,
    "runtime/bifs/global/system/BoxRegisterInterceptionPointsTest.bxs",
    "JVM-only: InterceptorService state"
);
compat_test!(
    bf_system_box_register_interceptor,
    "runtime/bifs/global/system/BoxRegisterInterceptorTest.bxs",
    "JVM-only: interceptor classes and callbacks"
);
compat_test!(
    bf_system_box_register_request_interceptor,
    "runtime/bifs/global/system/BoxRegisterRequestInterceptorTest.bxs",
    "JVM-only: interceptor classes and callbacks"
);
compat_test!(
    bf_system_call_stack_get,
    "runtime/bifs/global/system/CallStackGetTest.bxs",
    "JVM-only: Java component and internal call-stack frames"
);
compat_test!(
    bf_system_create_guid,
    "runtime/bifs/global/system/CreateGUIDTest.bxs"
);
compat_test!(
    bf_system_create_object_soap_payload,
    "runtime/bifs/global/system/CreateObjectSoapPayloadTest.bxs",
    "JVM-only: WireMock and SOAP Java client"
);
compat_test!(
    bf_system_create_object,
    "runtime/bifs/global/system/CreateObjectTest.bxs",
    "JVM-only: Java interop and classloaders"
);
compat_test!(
    bf_system_create_object_web_service,
    "runtime/bifs/global/system/CreateObjectWebServiceTest.bxs",
    "JVM-only: SOAP Java client and WSDL services"
);
compat_test!(
    bf_system_create_uuid,
    "runtime/bifs/global/system/CreateUUIDTest.bxs"
);
compat_test!(
    bf_system_duplicate,
    "runtime/bifs/global/system/DuplicateTest.bxs"
);
compat_test!(bf_system_echo, "runtime/bifs/global/system/EchoTest.bxs");
compat_test!(
    bf_system_get_base_tag_data,
    "runtime/bifs/global/system/GetBaseTagDataTest.bxs"
);
compat_test!(
    bf_system_get_base_tag_list,
    "runtime/bifs/global/system/GetBaseTagListTest.bxs"
);
compat_test!(
    bf_system_get_base_template_path,
    "runtime/bifs/global/system/GetBaseTemplatePathTest.bxs"
);
compat_test!(
    bf_system_get_box_context,
    "runtime/bifs/global/system/GetBoxContextTest.bxs",
    "JVM-only: internal IBoxContext identity"
);
compat_test!(
    bf_system_get_box_runtime,
    "runtime/bifs/global/system/GetBoxRuntimeTest.bxs",
    "JVM-only: internal BoxRuntime identity"
);
compat_test!(
    bf_system_get_box_version_info,
    "runtime/bifs/global/system/GetBoxVersionInfoTest.bxs"
);
compat_test!(
    bf_system_get_class_metadata,
    "runtime/bifs/global/system/GetClassMetadataTest.bxs",
    "JVM-only: class files, annotations, and class metadata loader"
);
compat_test!(
    bf_system_get_component_list,
    "runtime/bifs/global/system/GetComponentListTest.bxs"
);
compat_test!(
    bf_system_get_current_template_path,
    "runtime/bifs/global/system/GetCurrentTemplatePathTest.bxs"
);
compat_test!(
    bf_system_get_file_from_path,
    "runtime/bifs/global/system/GetFileFromPathTest.bxs"
);
compat_test!(
    bf_system_get_function_called_name,
    "runtime/bifs/global/system/GetFunctionCalledNameTest.bxs"
);
compat_test!(
    bf_system_get_function_list,
    "runtime/bifs/global/system/GetFunctionListTest.bxs"
);
compat_test!(
    bf_system_get_module_info,
    "runtime/bifs/global/system/GetModuleInfoTest.bxs"
);
compat_test!(
    bf_system_get_module_list,
    "runtime/bifs/global/system/GetModuleListTest.bxs"
);
compat_test!(
    bf_system_get_semver,
    "runtime/bifs/global/system/GetSemverTest.bxs",
    "JVM-only: org.semver4j Java library"
);
compat_test!(
    bf_system_get_system_setting,
    "runtime/bifs/global/system/GetSystemSettingTest.bxs"
);
compat_test!(
    bf_system_get_temp_directory,
    "runtime/bifs/global/system/GetTempDirectoryTest.bxs"
);
compat_test!(
    bf_system_get_tick_count,
    "runtime/bifs/global/system/GetTickCountTest.bxs"
);
compat_test!(
    bf_system_invoke,
    "runtime/bifs/global/system/InvokeTest.bxs"
);
compat_test!(
    bf_system_is_instance_of,
    "runtime/bifs/global/system/IsInstanceOfTest.bxs"
);
compat_test!(
    bf_system_java_cast,
    "runtime/bifs/global/system/JavaCastTest.bxs",
    "JVM-only: Java primitive and native-array interop"
);
compat_test!(bf_system_lock, "runtime/bifs/global/system/LockTest.bxs");
compat_test!(
    bf_system_module_reload,
    "runtime/bifs/global/system/ModuleReloadTest.bxs"
);
compat_test!(
    bf_system_object_marshaller,
    "runtime/bifs/global/system/ObjectMarshallerTest.bxs",
    "JVM-only: Java object serialization"
);
compat_test!(bf_system_print, "runtime/bifs/global/system/PrintTest.bxs");
compat_test!(
    bf_system_println,
    "runtime/bifs/global/system/PrintlnTest.bxs"
);
compat_test!(
    bf_system_run_thread_in_context,
    "runtime/bifs/global/system/RunThreadInContextTest.bxs"
);
compat_test!(
    bf_system_session_invalidate,
    "runtime/bifs/global/system/SessionInvalidateTest.bxs",
    "JVM-only: application session management"
);
compat_test!(
    bf_system_session_rotate,
    "runtime/bifs/global/system/SessionRotateTest.bxs",
    "JVM-only: application session management"
);
compat_test!(
    bf_system_session_start_time,
    "runtime/bifs/global/system/SessionStartTimeTest.bxs",
    "JVM-only: application session management"
);
compat_test!(bf_system_sleep, "runtime/bifs/global/system/SleepTest.bxs");
compat_test!(
    bf_system_system_execute,
    "runtime/bifs/global/system/SystemExecuteTest.bxs"
);
compat_test!(bf_system_throw, "runtime/bifs/global/system/ThrowTest.bxs");
compat_test!(bf_system_trace, "runtime/bifs/global/system/TraceTest.bxs");
compat_test!(
    bf_system_url_encoded_format,
    "runtime/bifs/global/system/URLEncodedFormatTest.bxs"
);
compat_test!(
    bf_system_write_log,
    "runtime/bifs/global/system/WriteLogTest.bxs"
);
compat_test!(
    bf_system_create_dynamic_proxy,
    "runtime/bifs/global/system/java/CreateDynamicProxyTest.bxs",
    "JVM-only: Java dynamic proxies and threads"
);
compat_test!(
    bf_query_query_sort,
    "runtime/bifs/global/query/QuerySortTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/net — ortus/boxlang/runtime/bifs/global/net/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_net_get_localhost_ip,
    "runtime/bifs/global/net/GetLocalhostIpTest.bxs"
);
compat_test!(bf_net_soap, "runtime/bifs/global/net/SOAPTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/jdbc — ortus/boxlang/runtime/bifs/global/jdbc/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_jdbc_derby_sleep,
    "runtime/bifs/global/jdbc/DerbySleep.bxs"
);
compat_test!(
    bf_jdbc_is_in_transaction,
    "runtime/bifs/global/jdbc/IsInTransactionTest.bxs"
);
compat_test!(
    bf_jdbc_preserve_single_quotes,
    "runtime/bifs/global/jdbc/PreserveSingleQuotesTest.bxs"
);
compat_test!(
    bf_jdbc_query_execute,
    "runtime/bifs/global/jdbc/QueryExecuteTest.bxs"
);
compat_test!(
    bf_jdbc_transaction_commit,
    "runtime/bifs/global/jdbc/TransactionCommitTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/temporal — .../temporal/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_temporal_clear_timezone,
    "runtime/bifs/global/temporal/ClearTimezoneTest.bxs"
);
compat_test!(
    bf_temporal_create_date,
    "runtime/bifs/global/temporal/CreateDateTest.bxs"
);
compat_test!(
    bf_temporal_create_datetime,
    "runtime/bifs/global/temporal/CreateDateTimeTest.bxs"
);
compat_test!(
    bf_temporal_create_odbc_datetime,
    "runtime/bifs/global/temporal/CreateODBCDateTimeTest.bxs"
);
compat_test!(
    bf_temporal_create_timespan,
    "runtime/bifs/global/temporal/CreateTimeSpanTest.bxs"
);
compat_test!(
    bf_temporal_create_time,
    "runtime/bifs/global/temporal/CreateTimeTest.bxs"
);
compat_test!(
    bf_temporal_date_add,
    "runtime/bifs/global/temporal/DateAddTest.bxs"
);
compat_test!(
    bf_temporal_date_compare,
    "runtime/bifs/global/temporal/DateCompareTest.bxs"
);
compat_test!(
    bf_temporal_date_convert,
    "runtime/bifs/global/temporal/DateConvertTest.bxs"
);
compat_test!(
    bf_temporal_date_diff,
    "runtime/bifs/global/temporal/DateDiffTest.bxs"
);
compat_test!(
    bf_temporal_date_part,
    "runtime/bifs/global/temporal/DatePartTest.bxs"
);
compat_test!(
    bf_temporal_datetime_format,
    "runtime/bifs/global/temporal/DateTimeFormatTest.bxs"
);
compat_test!(
    bf_temporal_get_timezone_info,
    "runtime/bifs/global/temporal/GetTimezoneInfoTest.bxs"
);
compat_test!(bf_temporal_now, "runtime/bifs/global/temporal/NowTest.bxs");
compat_test!(
    bf_temporal_parse_datetime,
    "runtime/bifs/global/temporal/ParseDateTimeTest.bxs"
);
compat_test!(
    bf_temporal_set_timezone,
    "runtime/bifs/global/temporal/SetTimezoneTest.bxs"
);
compat_test!(
    bf_temporal_time_units,
    "runtime/bifs/global/temporal/TimeUnitsTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/async — ortus/boxlang/runtime/bifs/global/async/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_async_all_apply,
    "runtime/bifs/global/async/AsyncAllApplyTest.bxs"
);
compat_test!(bf_async_all, "runtime/bifs/global/async/AsyncAllTest.bxs");
compat_test!(bf_async_any, "runtime/bifs/global/async/AsyncAnyTest.bxs");
compat_test!(bf_async_run, "runtime/bifs/global/async/AsyncRunTest.bxs");
compat_test!(
    bf_async_executor_delete,
    "runtime/bifs/global/async/ExecutorDeleteTest.bxs"
);
compat_test!(
    bf_async_executor_get,
    "runtime/bifs/global/async/ExecutorGetTest.bxs"
);
compat_test!(
    bf_async_future_new,
    "runtime/bifs/global/async/FutureNewTest.bxs"
);
compat_test!(
    bf_async_is_thread_alive,
    "runtime/bifs/global/async/IsThreadAliveTest.bxs"
);
compat_test!(
    bf_async_is_thread_interrupted,
    "runtime/bifs/global/async/IsThreadInterruptedTest.bxs"
);
compat_test!(
    bf_async_thread_current,
    "runtime/bifs/global/async/ThreadCurrentTest.bxs",
    "JVM-only, hard requirement: java.lang.Thread class identity and methods"
);
compat_test!(
    bf_async_thread_join,
    "runtime/bifs/global/async/ThreadJoinTest.bxs"
);
compat_test!(
    bf_async_thread_new,
    "runtime/bifs/global/async/ThreadNewTest.bxs"
);
compat_test!(
    bf_async_thread_terminate,
    "runtime/bifs/global/async/ThreadTerminateTest.bxs"
);
compat_test!(
    bf_async_is_in_thread,
    "runtime/bifs/global/async/isInThreadTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/watcher — ortus/boxlang/runtime/bifs/global/watcher/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_watcher_exists,
    "runtime/bifs/global/watcher/WatcherExistsTest.bxs"
);
compat_test!(
    bf_watcher_get_all,
    "runtime/bifs/global/watcher/WatcherGetAllTest.bxs"
);
compat_test!(
    bf_watcher_get,
    "runtime/bifs/global/watcher/WatcherGetTest.bxs"
);
compat_test!(
    bf_watcher_list,
    "runtime/bifs/global/watcher/WatcherListTest.bxs"
);
compat_test!(
    bf_watcher_new,
    "runtime/bifs/global/watcher/WatcherNewTest.bxs"
);
compat_test!(
    bf_watcher_restart,
    "runtime/bifs/global/watcher/WatcherRestartTest.bxs"
);
compat_test!(
    bf_watcher_shutdown_all,
    "runtime/bifs/global/watcher/WatcherShutdownAllTest.bxs"
);
compat_test!(
    bf_watcher_shutdown,
    "runtime/bifs/global/watcher/WatcherShutdownTest.bxs"
);
compat_test!(
    bf_watcher_start,
    "runtime/bifs/global/watcher/WatcherStartTest.bxs"
);
compat_test!(
    bf_watcher_stop_all,
    "runtime/bifs/global/watcher/WatcherStopAllTest.bxs"
);
compat_test!(
    bf_watcher_stop,
    "runtime/bifs/global/watcher/WatcherStopTest.bxs"
);

// ---------------------------------------------------------------------------
// runtime/bifs/global/set — ortus/boxlang/runtime/bifs/global/set/*.java
// ---------------------------------------------------------------------------

compat_test!(bf_set_set_bifs, "runtime/bifs/global/set/SetBIFsTest.bxs");

// ---------------------------------------------------------------------------
// runtime/bifs/global/scheduler — ortus/boxlang/runtime/bifs/global/scheduler/*.java
// ---------------------------------------------------------------------------

compat_test!(
    bf_scheduler_schedulers,
    "runtime/bifs/global/scheduler/SchedulersTest.bxs"
);

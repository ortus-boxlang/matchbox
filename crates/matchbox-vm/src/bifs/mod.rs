use crate::types::{BxNativeFunction, BxNativeObject, BxVM, BxValue, Tracer};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Timelike, Utc,
};
use rand::{RngExt, SeedableRng, rngs::StdRng};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MATH_RNG: &str = "__matchboxMathRng";
static UNSYNCHRONIZED_ARRAYS: OnceLock<Mutex<HashSet<(usize, usize)>>> = OnceLock::new();
static ARRAY_DIMENSIONS: OnceLock<Mutex<HashMap<(usize, usize), usize>>> = OnceLock::new();

#[derive(Debug)]
struct MathRng {
    rng: StdRng,
}

impl BxNativeObject for MathRng {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name {
            "random" => Ok(BxValue::new_number(self.rng.random::<f64>())),
            "randomrange" => {
                let [low, high] = args else {
                    return Err("randomrange requires two numbers".to_string());
                };
                if !low.is_number() || !high.is_number() {
                    return Err("randomrange requires two numbers".to_string());
                }
                let low = low.as_number();
                let high = high.as_number();
                let value = if low.fract() == 0.0 && high.fract() == 0.0 {
                    self.rng.random_range((low as i64)..=(high as i64)) as f64
                } else {
                    self.rng.random_range(low..=high)
                };
                Ok(BxValue::new_number(value))
            }
            "seed" => {
                let seed = args
                    .first()
                    .filter(|value| value.is_number())
                    .ok_or_else(|| "seed requires a number".to_string())?;
                self.rng = StdRng::seed_from_u64(seed.as_number() as u64);
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Math RNG method '{}' not found", name)),
        }
    }
}

fn math_rng(vm: &mut dyn BxVM) -> usize {
    vm.get_global_value(MATH_RNG)
        .and_then(|value| value.as_gc_id())
        .unwrap_or_else(|| {
            let id = vm.native_object_new(Rc::new(RefCell::new(MathRng {
                rng: StdRng::seed_from_u64(0),
            })));
            vm.insert_global(MATH_RNG.to_string(), BxValue::new_ptr(id));
            id
        })
}

#[cfg(feature = "bif-jni")]
mod jni;
#[cfg(not(feature = "bif-jni"))]
mod jni {
    use crate::types::{BxVM, BxValue};
    pub fn create_java_object(
        _vm: &mut dyn BxVM,
        _class_name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err("Java interoperability is not enabled in this build.".to_string())
    }
}
mod async_runtime;
mod binary;
mod cache;
mod cli;
mod conversion;
mod crypto;
#[cfg(feature = "bif-datasource")]
mod datasource;
mod fs;
mod http;
mod i18n;
mod json;
mod list_query_extra;
mod math_datetime;
mod net;
mod set;
mod system;
mod system_execute;
mod system_output;
mod toml;
mod type_format;
mod watcher;
pub(crate) mod xml;
mod yaml;
mod zip;

pub fn register_all() -> HashMap<String, BxNativeFunction> {
    let mut bifs = HashMap::new();

    bifs.insert(
        "systemexecute".to_string(),
        system_execute::system_execute as BxNativeFunction,
    );
    bifs.insert(
        "systemoutput".to_string(),
        system_output::system_output as BxNativeFunction,
    );
    bifs.insert(
        "boxannounce".to_string(),
        system::box_announce as BxNativeFunction,
    );
    bifs.insert("boxast".to_string(), system::box_ast as BxNativeFunction);
    bifs.insert(
        "boxmodulereload".to_string(),
        system::box_module_reload as BxNativeFunction,
    );
    bifs.insert("trace".to_string(), system::trace as BxNativeFunction);
    bifs.insert(
        "writelog".to_string(),
        system::write_log as BxNativeFunction,
    );
    bifs.insert(
        "getfunctioncalledname".to_string(),
        system::get_function_called_name as BxNativeFunction,
    );
    bifs.insert(
        "getboxcontext".to_string(),
        system::get_box_context as BxNativeFunction,
    );
    bifs.insert(
        "runthreadincontext".to_string(),
        system::run_thread_in_context as BxNativeFunction,
    );
    bifs.insert("lock".to_string(), system::lock as BxNativeFunction);
    bifs.insert(
        "getbasetagdata".to_string(),
        system::get_base_tag_data as BxNativeFunction,
    );
    bifs.insert(
        "getbasetaglist".to_string(),
        system::get_base_tag_list as BxNativeFunction,
    );
    bifs.insert(
        "getbasetemplatepath".to_string(),
        system::get_base_template_path as BxNativeFunction,
    );
    bifs.insert(
        "getcurrenttemplatepath".to_string(),
        system::get_current_template_path as BxNativeFunction,
    );
    bifs.insert(
        "getboxversioninfo".to_string(),
        system::get_box_version_info as BxNativeFunction,
    );
    bifs.insert(
        "getcomponentlist".to_string(),
        system::get_component_list as BxNativeFunction,
    );
    bifs.insert(
        "getfunctionlist".to_string(),
        system::get_function_list as BxNativeFunction,
    );
    bifs.insert(
        "getmoduleinfo".to_string(),
        system::get_module_info as BxNativeFunction,
    );
    bifs.insert(
        "getmodulelist".to_string(),
        system::get_module_list as BxNativeFunction,
    );
    bifs.insert("invoke".to_string(), system::invoke as BxNativeFunction);
    bifs.insert(
        "urlencodedformat".to_string(),
        system::url_encoded_format as BxNativeFunction,
    );
    bifs.insert(
        "getfilefrompath".to_string(),
        system::get_file_from_path as BxNativeFunction,
    );

    // Math BIFs
    bifs.insert("round".to_string(), round as BxNativeFunction);
    bifs.insert("floor".to_string(), floor_bif as BxNativeFunction);
    bifs.insert("int".to_string(), int_bif as BxNativeFunction);
    bifs.insert("ceiling".to_string(), ceiling_bif as BxNativeFunction);
    bifs.insert("abs".to_string(), abs_bif as BxNativeFunction);
    bifs.insert("min".to_string(), min_bif as BxNativeFunction);
    bifs.insert("max".to_string(), max_bif as BxNativeFunction);
    bifs.insert("randrange".to_string(), rand_range as BxNativeFunction);
    bifs.insert("rand".to_string(), rand as BxNativeFunction);
    bifs.insert("randomize".to_string(), randomize as BxNativeFunction);
    bifs.insert("pi".to_string(), pi as BxNativeFunction);
    bifs.insert("log".to_string(), log_bif as BxNativeFunction);
    bifs.insert("log10".to_string(), log10_bif as BxNativeFunction);
    bifs.insert("exp".to_string(), exp_bif as BxNativeFunction);
    bifs.insert("sin".to_string(), sin_bif as BxNativeFunction);
    bifs.insert("cos".to_string(), cos_bif as BxNativeFunction);
    bifs.insert("tan".to_string(), tan_bif as BxNativeFunction);
    bifs.insert("asin".to_string(), asin_bif as BxNativeFunction);
    bifs.insert("acos".to_string(), acos_bif as BxNativeFunction);
    bifs.insert("atan".to_string(), atan_bif as BxNativeFunction);
    bifs.insert("atn".to_string(), atan_bif as BxNativeFunction);
    bifs.insert("atan2".to_string(), atan2_bif as BxNativeFunction);

    // Array BIFs
    bifs.insert("arrayappend".to_string(), array_append as BxNativeFunction);
    bifs.insert(
        "arrayappendmember".to_string(),
        array_append_member as BxNativeFunction,
    );
    bifs.insert("arraymap".to_string(), array_map as BxNativeFunction);
    bifs.insert("arraylen".to_string(), len as BxNativeFunction);
    bifs.insert(
        "arrayprepend".to_string(),
        array_prepend as BxNativeFunction,
    );
    bifs.insert("arraynew".to_string(), array_new as BxNativeFunction);
    bifs.insert(
        "arrayissynchronized".to_string(),
        array_is_synchronized_bif as BxNativeFunction,
    );
    bifs.insert("arraypop".to_string(), array_pop_bif as BxNativeFunction);
    bifs.insert(
        "arraydeleteat".to_string(),
        array_delete_at_bif as BxNativeFunction,
    );
    bifs.insert(
        "arrayinsertat".to_string(),
        array_insert_at_bif as BxNativeFunction,
    );
    bifs.insert(
        "arrayclear".to_string(),
        array_clear_bif as BxNativeFunction,
    );
    bifs.insert("arrayset".to_string(), array_set_bif as BxNativeFunction);
    bifs.insert("arrayswap".to_string(), array_swap_bif as BxNativeFunction);
    bifs.insert(
        "arrayresize".to_string(),
        array_resize_bif as BxNativeFunction,
    );
    bifs.insert("bytesnew".to_string(), bytes_new as BxNativeFunction);
    bifs.insert("byteslen".to_string(), bytes_len_bif as BxNativeFunction);
    bifs.insert("bytesget".to_string(), bytes_get_bif as BxNativeFunction);
    bifs.insert("bytesset".to_string(), bytes_set_bif as BxNativeFunction);
    bifs.insert("isbinary".to_string(), is_bytes_bif as BxNativeFunction);

    // Struct BIFs
    bifs.insert("structnew".to_string(), struct_new as BxNativeFunction);
    bifs.insert(
        "structinsert".to_string(),
        struct_insert_bif as BxNativeFunction,
    );
    bifs.insert(
        "structupdate".to_string(),
        struct_update_bif as BxNativeFunction,
    );
    bifs.insert(
        "structdelete".to_string(),
        struct_delete_bif as BxNativeFunction,
    );
    bifs.insert(
        "structkeyexists".to_string(),
        struct_key_exists_bif as BxNativeFunction,
    );
    bifs.insert("structget".to_string(), struct_get_bif as BxNativeFunction);
    bifs.insert(
        "structkeyarray".to_string(),
        struct_key_array_bif as BxNativeFunction,
    );
    bifs.insert(
        "structclear".to_string(),
        struct_clear_bif as BxNativeFunction,
    );
    bifs.insert(
        "structfind".to_string(),
        struct_find_bif as BxNativeFunction,
    );
    bifs.insert(
        "structisempty".to_string(),
        struct_is_empty_bif as BxNativeFunction,
    );
    bifs.insert("structcount".to_string(), len as BxNativeFunction);
    bifs.insert(
        "structiscasesensitive".to_string(),
        struct_is_case_sensitive_bif as BxNativeFunction,
    );
    bifs.insert(
        "structisordered".to_string(),
        struct_is_ordered_bif as BxNativeFunction,
    );
    bifs.insert(
        "structequals".to_string(),
        struct_equals_bif as BxNativeFunction,
    );
    bifs.insert(
        "structgetmetadata".to_string(),
        struct_get_metadata_bif as BxNativeFunction,
    );
    bifs.insert(
        "structtoquerystring".to_string(),
        struct_to_query_string_bif as BxNativeFunction,
    );
    bifs.insert(
        "structtosorted".to_string(),
        struct_to_sorted_bif as BxNativeFunction,
    );
    bifs.insert(
        "structkeytranslate".to_string(),
        struct_key_translate_bif as BxNativeFunction,
    );
    bifs.insert(
        "structfindkey".to_string(),
        struct_find_key_bif as BxNativeFunction,
    );

    // StringBuilder BIFs
    bifs.insert(
        "stringbuildernew".to_string(),
        string_builder_new_bif as BxNativeFunction,
    );
    bifs.insert(
        "isstringbuilder".to_string(),
        is_string_builder_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderappend".to_string(),
        string_builder_append_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderclear".to_string(),
        string_builder_clear_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuildercontains".to_string(),
        string_builder_contains_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderdelete".to_string(),
        string_builder_delete_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderendswith".to_string(),
        string_builder_ends_with_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderfind".to_string(),
        string_builder_find_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderinsert".to_string(),
        string_builder_insert_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderleft".to_string(),
        string_builder_left_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuildermid".to_string(),
        string_builder_mid_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderprepend".to_string(),
        string_builder_prepend_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderreplace".to_string(),
        string_builder_replace_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderreverse".to_string(),
        string_builder_reverse_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderright".to_string(),
        string_builder_right_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuilderstartswith".to_string(),
        string_builder_starts_with_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringbuildertrim".to_string(),
        string_builder_trim_bif as BxNativeFunction,
    );

    // Core BIFs
    bifs.insert("len".to_string(), len as BxNativeFunction);
    bifs.insert(
        "getmetadata".to_string(),
        get_metadata_bif as BxNativeFunction,
    );
    bifs.insert(
        "writeoutput".to_string(),
        write_output_bif as BxNativeFunction,
    );
    bifs.insert("echo".to_string(), write_output_bif as BxNativeFunction);
    bifs.insert(
        "preservesinglequotes".to_string(),
        preserve_single_quotes_bif as BxNativeFunction,
    );
    bifs.insert("duplicate".to_string(), duplicate_bif as BxNativeFunction);
    bifs.insert(
        "createobject".to_string(),
        create_object as BxNativeFunction,
    );
    bifs.insert("isnull".to_string(), is_null_bif as BxNativeFunction);
    bifs.insert("nullvalue".to_string(), null_value_bif as BxNativeFunction);
    bifs.insert("isnumeric".to_string(), is_numeric_bif as BxNativeFunction);
    bifs.insert(
        "lsisnumeric".to_string(),
        is_numeric_bif as BxNativeFunction,
    );
    bifs.insert("isarray".to_string(), is_array_bif as BxNativeFunction);
    bifs.insert("isstruct".to_string(), is_struct_bif as BxNativeFunction);
    bifs.insert("isboolean".to_string(), is_boolean_bif as BxNativeFunction);
    bifs.insert("isstring".to_string(), is_string_bif as BxNativeFunction);
    bifs.insert("isdate".to_string(), is_date_bif as BxNativeFunction);
    bifs.insert("isnumericdate".to_string(), is_date_bif as BxNativeFunction);
    bifs.insert(
        "issimplevalue".to_string(),
        is_simple_value_bif as BxNativeFunction,
    );
    bifs.insert("isobject".to_string(), is_object_bif as BxNativeFunction);
    bifs.insert(
        "isinstanceof".to_string(),
        is_instance_of_bif as BxNativeFunction,
    );
    bifs.insert("ucase".to_string(), ucase as BxNativeFunction);
    bifs.insert("lcase".to_string(), lcase as BxNativeFunction);
    bifs.insert("trim".to_string(), trim_bif as BxNativeFunction);
    bifs.insert("ltrim".to_string(), ltrim_bif as BxNativeFunction);
    bifs.insert("rtrim".to_string(), rtrim_bif as BxNativeFunction);
    bifs.insert("compare".to_string(), compare_bif as BxNativeFunction);
    bifs.insert(
        "comparenocase".to_string(),
        compare_no_case_bif as BxNativeFunction,
    );
    bifs.insert(
        "removechars".to_string(),
        remove_chars_bif as BxNativeFunction,
    );
    bifs.insert("stripcr".to_string(), strip_cr_bif as BxNativeFunction);
    bifs.insert("ucfirst".to_string(), uc_first_bif as BxNativeFunction);
    bifs.insert(
        "replacenocase".to_string(),
        replace_no_case_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringendswithnocase".to_string(),
        string_ends_with_no_case_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringstartswithnocase".to_string(),
        string_starts_with_no_case_bif as BxNativeFunction,
    );
    bifs.insert("ascii".to_string(), ascii_bif as BxNativeFunction);
    bifs.insert("findoneof".to_string(), find_one_of_bif as BxNativeFunction);
    bifs.insert("insert".to_string(), insert_bif as BxNativeFunction);
    bifs.insert("reescape".to_string(), re_escape_bif as BxNativeFunction);
    bifs.insert(
        "yesnoformat".to_string(),
        yes_no_format_bif as BxNativeFunction,
    );
    bifs.insert("snakecase".to_string(), snake_case_bif as BxNativeFunction);
    bifs.insert("kebabcase".to_string(), kebab_case_bif as BxNativeFunction);
    bifs.insert("camelcase".to_string(), camel_case_bif as BxNativeFunction);
    bifs.insert(
        "pascalcase".to_string(),
        pascal_case_bif as BxNativeFunction,
    );
    bifs.insert(
        "replacelist".to_string(),
        replace_list_bif as BxNativeFunction,
    );
    bifs.insert(
        "repeatstring".to_string(),
        repeat_string as BxNativeFunction,
    );
    bifs.insert("find".to_string(), find_bif as BxNativeFunction);
    bifs.insert(
        "findnocase".to_string(),
        find_no_case_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringfind".to_string(),
        string_find_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringfindnocase".to_string(),
        string_find_no_case_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringendswith".to_string(),
        string_ends_with_bif as BxNativeFunction,
    );
    bifs.insert(
        "stringstartswith".to_string(),
        string_starts_with_bif as BxNativeFunction,
    );
    bifs.insert("val".to_string(), val_bif as BxNativeFunction);
    bifs.insert("left".to_string(), left_bif as BxNativeFunction);
    bifs.insert("right".to_string(), right_bif as BxNativeFunction);
    bifs.insert("reverse".to_string(), reverse_bif as BxNativeFunction);
    bifs.insert(
        "spanexcluding".to_string(),
        span_excluding_bif as BxNativeFunction,
    );
    bifs.insert(
        "spanincluding".to_string(),
        span_including_bif as BxNativeFunction,
    );
    bifs.insert(
        "jsstringformat".to_string(),
        js_string_format_bif as BxNativeFunction,
    );
    bifs.insert("ljustify".to_string(), l_justify_bif as BxNativeFunction);
    bifs.insert("rjustify".to_string(), r_justify_bif as BxNativeFunction);
    bifs.insert(
        "paragraphformat".to_string(),
        paragraph_format_bif as BxNativeFunction,
    );
    bifs.insert("slugify".to_string(), slugify_bif as BxNativeFunction);
    bifs.insert("wrap".to_string(), wrap_bif as BxNativeFunction);
    bifs.insert(
        "stringbind".to_string(),
        string_bind_bif as BxNativeFunction,
    );
    bifs.insert(
        "querystringtostruct".to_string(),
        query_string_to_struct_bif as BxNativeFunction,
    );
    bifs.insert(
        "charsetdecode".to_string(),
        charset_decode_bif as BxNativeFunction,
    );
    bifs.insert(
        "charsetencode".to_string(),
        charset_encode_bif as BxNativeFunction,
    );
    bifs.insert(
        "sqlprettify".to_string(),
        sql_prettify_bif as BxNativeFunction,
    );
    bifs.insert("tostring".to_string(), to_string_bif as BxNativeFunction);
    bifs.insert("stringlen".to_string(), len as BxNativeFunction);
    bifs.insert("cast".to_string(), cast_bif as BxNativeFunction);
    bifs.insert("now".to_string(), now as BxNativeFunction);
    bifs.insert("createdate".to_string(), create_date as BxNativeFunction);
    bifs.insert(
        "createdatetime".to_string(),
        create_date_time as BxNativeFunction,
    );
    bifs.insert("dateadd".to_string(), date_add as BxNativeFunction);
    bifs.insert("datediff".to_string(), date_diff as BxNativeFunction);
    bifs.insert(
        "toepochmillis".to_string(),
        to_epoch_millis as BxNativeFunction,
    );
    bifs.insert(
        "timeformat".to_string(),
        time_format_bif as BxNativeFunction,
    );
    bifs.insert("offset".to_string(), offset_bif as BxNativeFunction);
    bifs.insert(
        "gettimezone".to_string(),
        get_timezone_bif as BxNativeFunction,
    );
    bifs.insert(
        "getnumericdate".to_string(),
        get_numeric_date_bif as BxNativeFunction,
    );
    bifs.insert("gettime".to_string(), get_time_bif as BxNativeFunction);
    bifs.insert(
        "dateformat".to_string(),
        date_format_bif as BxNativeFunction,
    );
    bifs.insert(
        "datetimeformat".to_string(),
        date_time_format_bif as BxNativeFunction,
    );
    bifs.insert(
        "parsedatetime".to_string(),
        parse_date_time_bif as BxNativeFunction,
    );
    bifs.insert("listtoarray".to_string(), list_to_array as BxNativeFunction);
    bifs.insert("listlen".to_string(), list_len as BxNativeFunction);
    bifs.insert("listgetat".to_string(), list_get_at as BxNativeFunction);
    bifs.insert("listappend".to_string(), list_append as BxNativeFunction);
    bifs.insert("listfirst".to_string(), list_first as BxNativeFunction);
    bifs.insert("listlast".to_string(), list_last as BxNativeFunction);
    bifs.insert("listrest".to_string(), list_rest as BxNativeFunction);
    bifs.insert(
        "listdeleteat".to_string(),
        list_delete_at as BxNativeFunction,
    );
    bifs.insert("listfind".to_string(), list_find as BxNativeFunction);
    bifs.insert(
        "listfindnocase".to_string(),
        list_find_no_case as BxNativeFunction,
    );
    bifs.insert("listsort".to_string(), list_sort as BxNativeFunction);
    bifs.insert("indexof".to_string(), index_of as BxNativeFunction);
    bifs.insert("rematch".to_string(), re_match as BxNativeFunction);
    bifs.insert("mid".to_string(), mid_bif as BxNativeFunction);
    bifs.insert("replace".to_string(), replace_bif as BxNativeFunction);
    bifs.insert("chr".to_string(), chr_bif as BxNativeFunction);
    bifs.insert("char".to_string(), chr_bif as BxNativeFunction);
    bifs.insert(
        "futureonerror".to_string(),
        future_on_error as BxNativeFunction,
    );
    bifs.insert("rematch".to_string(), re_match as BxNativeFunction);
    bifs.insert(
        "rematchnocase".to_string(),
        re_match_no_case as BxNativeFunction,
    );
    bifs.insert("refind".to_string(), re_find as BxNativeFunction);
    bifs.insert(
        "refindnocase".to_string(),
        re_find_no_case as BxNativeFunction,
    );
    bifs.insert("rereplace".to_string(), re_replace as BxNativeFunction);
    bifs.insert(
        "rereplacenocase".to_string(),
        re_replace_no_case as BxNativeFunction,
    );

    // Crypto BIFs
    #[cfg(feature = "bif-crypto")]
    {
        bifs.insert("hash".to_string(), crypto::hash_bif as BxNativeFunction);
        bifs.insert("hash40".to_string(), crypto::hash40_bif as BxNativeFunction);
        bifs.insert("hmac".to_string(), crypto::hmac_bif as BxNativeFunction);
        bifs.insert(
            "generatesecretkey".to_string(),
            crypto::generate_secret_key as BxNativeFunction,
        );
        bifs.insert(
            "generatepbkdfkey".to_string(),
            crypto::generate_pbkdf_key as BxNativeFunction,
        );
        bifs.insert(
            "encrypt".to_string(),
            crypto::encrypt_bif as BxNativeFunction,
        );
        bifs.insert(
            "decrypt".to_string(),
            crypto::decrypt_bif as BxNativeFunction,
        );
    }

    // System BIFs
    bifs.insert("createuuid".to_string(), create_uuid as BxNativeFunction);
    bifs.insert("createguid".to_string(), create_guid as BxNativeFunction);
    bifs.insert(
        "getsystemsetting".to_string(),
        get_system_setting as BxNativeFunction,
    );

    // Date/Time BIFs
    bifs.insert("now".to_string(), now as BxNativeFunction);
    bifs.insert(
        "gettickcount".to_string(),
        get_tick_count as BxNativeFunction,
    );
    bifs.insert("sleep".to_string(), sleep as BxNativeFunction);
    bifs.insert("yield".to_string(), bx_yield as BxNativeFunction);

    // Extra Math & Date/Time BIFs
    math_datetime::register_math_datetime_bifs(&mut bifs);

    // CLI BIFs
    #[cfg(feature = "bif-cli")]
    {
        bifs.insert("cliclear".to_string(), cli::cli_clear as BxNativeFunction);
        bifs.insert("cliexit".to_string(), cli::cli_exit as BxNativeFunction);
        bifs.insert("exit".to_string(), cli::cli_exit as BxNativeFunction);
        bifs.insert(
            "cligetargs".to_string(),
            cli::cli_get_args as BxNativeFunction,
        );
        bifs.insert("cliread".to_string(), cli::cli_read as BxNativeFunction);
        bifs.insert(
            "cliisterminal".to_string(),
            cli::cli_is_terminal as BxNativeFunction,
        );
        bifs.insert(
            "clireadkey".to_string(),
            cli::cli_read_key as BxNativeFunction,
        );
        bifs.insert(
            "clireadpassword".to_string(),
            cli::cli_read_password as BxNativeFunction,
        );
        bifs.insert(
            "cliconfirm".to_string(),
            cli::cli_confirm as BxNativeFunction,
        );
        #[cfg(feature = "bif-cli")]
        bifs.insert("cliselect".to_string(), cli::cli_select as BxNativeFunction);
    }

    // Async BIFs
    bifs.insert("runasync".to_string(), run_async as BxNativeFunction);
    async_runtime::register_async_bifs(&mut bifs);

    // IO BIFs
    #[cfg(feature = "bif-io")]
    {
        bifs.insert(
            "directoryexists".to_string(),
            fs::directory_exists as BxNativeFunction,
        );
        bifs.insert(
            "directorycreate".to_string(),
            fs::directory_create as BxNativeFunction,
        );
        bifs.insert(
            "directorydelete".to_string(),
            fs::directory_delete as BxNativeFunction,
        );
        bifs.insert(
            "directorylist".to_string(),
            fs::directory_list as BxNativeFunction,
        );
        bifs.insert(
            "fileexists".to_string(),
            fs::file_exists as BxNativeFunction,
        );
        bifs.insert(
            "filedelete".to_string(),
            fs::file_delete as BxNativeFunction,
        );
        bifs.insert("filemove".to_string(), fs::file_move as BxNativeFunction);
        bifs.insert(
            "filepublishexclusive".to_string(),
            fs::file_publish_exclusive as BxNativeFunction,
        );
        bifs.insert("filecopy".to_string(), fs::file_copy as BxNativeFunction);
        bifs.insert("fileinfo".to_string(), fs::file_info as BxNativeFunction);
        bifs.insert("getfileinfo".to_string(), fs::file_info as BxNativeFunction);
        bifs.insert(
            "filecreatesymlink".to_string(),
            fs::file_create_symlink as BxNativeFunction,
        );
        bifs.insert(
            "filesetexecutable".to_string(),
            fs::file_set_executable as BxNativeFunction,
        );
        bifs.insert("fileread".to_string(), fs::file_read as BxNativeFunction);
        bifs.insert("filewrite".to_string(), fs::file_write as BxNativeFunction);
        bifs.insert(
            "filewriteexclusive".to_string(),
            fs::file_write_exclusive as BxNativeFunction,
        );
        bifs.insert(
            "fileappend".to_string(),
            fs::file_append as BxNativeFunction,
        );
        bifs.insert(
            "contractpath".to_string(),
            fs::contract_path as BxNativeFunction,
        );
        bifs.insert(
            "createtempdirectory".to_string(),
            fs::create_temp_directory as BxNativeFunction,
        );
        bifs.insert(
            "gettempdirectory".to_string(),
            fs::get_temp_directory as BxNativeFunction,
        );
        bifs.insert(
            "createtempfile".to_string(),
            fs::create_temp_file as BxNativeFunction,
        );
        bifs.insert(
            "gettempfile".to_string(),
            fs::create_temp_file as BxNativeFunction,
        );
        bifs.insert(
            "directorycopy".to_string(),
            fs::directory_copy as BxNativeFunction,
        );
        bifs.insert(
            "directorymove".to_string(),
            fs::directory_move as BxNativeFunction,
        );
        bifs.insert(
            "directoryrename".to_string(),
            fs::directory_move as BxNativeFunction,
        );
        bifs.insert(
            "expandpath".to_string(),
            fs::expand_path as BxNativeFunction,
        );
        bifs.insert("fileclose".to_string(), fs::file_close as BxNativeFunction);
        bifs.insert(
            "filegetmimetype".to_string(),
            fs::file_get_mime_type as BxNativeFunction,
        );
        bifs.insert("fileiseof".to_string(), fs::file_is_eof as BxNativeFunction);
        bifs.insert("fileopen".to_string(), fs::file_open as BxNativeFunction);
        bifs.insert(
            "filereadline".to_string(),
            fs::file_read_line as BxNativeFunction,
        );
        bifs.insert("fileseek".to_string(), fs::file_seek as BxNativeFunction);
        bifs.insert(
            "filesetaccessmode".to_string(),
            fs::file_set_access_mode as BxNativeFunction,
        );
        bifs.insert(
            "filesetattribute".to_string(),
            fs::file_set_attribute as BxNativeFunction,
        );
        bifs.insert(
            "filesetlastmodified".to_string(),
            fs::file_set_last_modified as BxNativeFunction,
        );
        bifs.insert(
            "filewriteline".to_string(),
            fs::file_write_line as BxNativeFunction,
        );
        bifs.insert(
            "getcanonicalpath".to_string(),
            fs::get_canonical_path as BxNativeFunction,
        );
        bifs.insert(
            "getdirectoryfrompath".to_string(),
            fs::get_directory_from_path as BxNativeFunction,
        );
        bifs.insert(
            "propertyfile".to_string(),
            fs::property_file as BxNativeFunction,
        );
    }

    // HTTP BIFs
    #[cfg(feature = "bif-http")]
    bifs.insert("http".to_string(), http::http_bif as BxNativeFunction);

    // ZIP BIFs
    #[cfg(feature = "bif-zip")]
    {
        bifs.insert("extract".to_string(), zip::zip_extract as BxNativeFunction);
        bifs.insert(
            "compress".to_string(),
            zip::zip_compress as BxNativeFunction,
        );
        bifs.insert(
            "iszipfile".to_string(),
            zip::zip_is_zip_file as BxNativeFunction,
        );
    }

    // JSON BIFs
    bifs.insert(
        "deserializejson".to_string(),
        json::json_deserialize as BxNativeFunction,
    );
    bifs.insert(
        "deserializeyaml".to_string(),
        yaml::yaml_deserialize as BxNativeFunction,
    );
    bifs.insert(
        "deserializetoml".to_string(),
        toml::toml_deserialize as BxNativeFunction,
    );
    bifs.insert(
        "jsondeserialize".to_string(),
        json::json_deserialize as BxNativeFunction,
    );
    bifs.insert(
        "serializejson".to_string(),
        json::json_serialize as BxNativeFunction,
    );
    bifs.insert(
        "jsonserialize".to_string(),
        json::json_serialize as BxNativeFunction,
    );
    bifs.insert("isjson".to_string(), json::is_json as BxNativeFunction);
    bifs.insert(
        "loadproperties".to_string(),
        json::load_properties as BxNativeFunction,
    );

    // Conversion BIFs
    bifs.insert(
        "datanavigate".to_string(),
        conversion::data_navigate as BxNativeFunction,
    );
    bifs.insert(
        "jsonprettify".to_string(),
        conversion::json_prettify as BxNativeFunction,
    );
    bifs.insert(
        "parsenumber".to_string(),
        conversion::parse_number as BxNativeFunction,
    );
    bifs.insert(
        "tobase64".to_string(),
        conversion::to_base64 as BxNativeFunction,
    );
    bifs.insert(
        "tobinary".to_string(),
        conversion::to_binary as BxNativeFunction,
    );
    bifs.insert(
        "tomodifiable".to_string(),
        conversion::to_modifiable as BxNativeFunction,
    );
    bifs.insert(
        "tonumeric".to_string(),
        conversion::to_numeric as BxNativeFunction,
    );
    bifs.insert(
        "toscript".to_string(),
        conversion::to_script as BxNativeFunction,
    );
    bifs.insert(
        "tounmodifiable".to_string(),
        conversion::to_unmodifiable as BxNativeFunction,
    );

    // Datasource BIFs
    #[cfg(feature = "bif-datasource")]
    {
        bifs.insert(
            "datasourceregister".to_string(),
            datasource::datasource_register as BxNativeFunction,
        );
        bifs.insert(
            "datasourceunregister".to_string(),
            datasource::datasource_unregister as BxNativeFunction,
        );
        bifs.insert(
            "queryexecute".to_string(),
            datasource::query_execute as BxNativeFunction,
        );
        bifs.insert(
            "querynew".to_string(),
            datasource::query_new as BxNativeFunction,
        );
        bifs.insert(
            "queryaddrow".to_string(),
            datasource::query_add_row as BxNativeFunction,
        );
        bifs.insert(
            "querycolumndata".to_string(),
            datasource::query_column_data as BxNativeFunction,
        );
        bifs.insert(
            "querycolumnlist".to_string(),
            datasource::query_column_list as BxNativeFunction,
        );
        bifs.insert(
            "queryclear".to_string(),
            datasource::query_clear as BxNativeFunction,
        );
        bifs.insert(
            "querycolumnarray".to_string(),
            datasource::query_column_array as BxNativeFunction,
        );
        bifs.insert(
            "querycolumncount".to_string(),
            datasource::query_column_count as BxNativeFunction,
        );
        bifs.insert(
            "querycolumnexists".to_string(),
            datasource::query_column_exists as BxNativeFunction,
        );
        bifs.insert(
            "querykeyexists".to_string(),
            datasource::query_key_exists as BxNativeFunction,
        );
        bifs.insert(
            "queryrecordcount".to_string(),
            datasource::query_record_count as BxNativeFunction,
        );
        bifs.insert(
            "queryrowdata".to_string(),
            datasource::query_row_data as BxNativeFunction,
        );
        bifs.insert(
            "queryreverse".to_string(),
            datasource::query_reverse as BxNativeFunction,
        );
        bifs.insert(
            "queryrowswap".to_string(),
            datasource::query_row_swap as BxNativeFunction,
        );
        bifs.insert(
            "queryslice".to_string(),
            datasource::query_slice as BxNativeFunction,
        );
        bifs.insert(
            "querydeletecolumn".to_string(),
            datasource::query_delete_column as BxNativeFunction,
        );
        bifs.insert(
            "querydeleterow".to_string(),
            datasource::query_delete_row as BxNativeFunction,
        );
        bifs.insert(
            "queryaddcolumn".to_string(),
            datasource::query_add_column as BxNativeFunction,
        );
        bifs.insert(
            "queryappend".to_string(),
            datasource::query_append as BxNativeFunction,
        );
        bifs.insert(
            "queryprepend".to_string(),
            datasource::query_prepend as BxNativeFunction,
        );
        bifs.insert(
            "querygetcell".to_string(),
            datasource::query_get_cell as BxNativeFunction,
        );
        bifs.insert(
            "querysetcell".to_string(),
            datasource::query_set_cell as BxNativeFunction,
        );
        bifs.insert(
            "queryinsertat".to_string(),
            datasource::query_insert_at as BxNativeFunction,
        );
        bifs.insert(
            "querysetrow".to_string(),
            datasource::query_set_row as BxNativeFunction,
        );
        bifs.insert(
            "querycurrentrow".to_string(),
            datasource::query_current_row as BxNativeFunction,
        );
        bifs.insert(
            "querygetresult".to_string(),
            datasource::query_get_result as BxNativeFunction,
        );
        bifs.insert(
            "transactionbegin".to_string(),
            datasource::transaction_begin as BxNativeFunction,
        );
        bifs.insert(
            "transactioncommit".to_string(),
            datasource::transaction_commit as BxNativeFunction,
        );
        bifs.insert(
            "transactionrollback".to_string(),
            datasource::transaction_rollback as BxNativeFunction,
        );
        bifs.insert(
            "iswithintransaction".to_string(),
            datasource::is_within_transaction as BxNativeFunction,
        );
        bifs.insert(
            "isintransaction".to_string(),
            datasource::is_in_transaction as BxNativeFunction,
        );
    }

    // Extra module registrations
    list_query_extra::register_list_query_extra_bifs(&mut bifs);
    type_format::register_type_format_bifs(&mut bifs);
    binary::register_binary_bifs(&mut bifs);
    set::register_set_bifs(&mut bifs);
    i18n::register_i18n_bifs(&mut bifs);
    watcher::register_watcher_bifs(&mut bifs);
    xml::register_xml_bifs(&mut bifs);
    cache::register_cache_bifs(&mut bifs);
    net::register_net_bifs(&mut bifs);

    bifs
}

// --- Implementation ---

fn preserve_single_quotes_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    args.first()
        .copied()
        .ok_or_else(|| "preserveSingleQuotes() expects a string argument".to_string())
}

#[derive(Debug)]
struct StringBuilderObject {
    value: String,
}

impl BxNativeObject for StringBuilderObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let name = name.to_ascii_lowercase();
        let self_value = || BxValue::new_ptr(id);
        match name.as_str() {
            "__is_string_builder" => Ok(BxValue::new_bool(true)),
            "tostring" => Ok(BxValue::new_ptr(vm.string_new(self.value.clone()))),
            "append" => {
                self.value.push_str(&builder_arg_string(vm, args, 0)?);
                Ok(self_value())
            }
            "prepend" => {
                self.value = format!("{}{}", builder_arg_string(vm, args, 0)?, self.value);
                Ok(self_value())
            }
            "clear" => {
                self.value.clear();
                Ok(self_value())
            }
            "contains" => Ok(BxValue::new_bool(
                self.value.contains(&builder_arg_string(vm, args, 0)?),
            )),
            "containsnocase" => {
                Ok(BxValue::new_bool(self.value.to_lowercase().contains(
                    &builder_arg_string(vm, args, 0)?.to_lowercase(),
                )))
            }
            "startswith" => Ok(BxValue::new_bool(
                self.value.starts_with(&builder_arg_string(vm, args, 0)?),
            )),
            "endswith" => Ok(BxValue::new_bool(
                self.value.ends_with(&builder_arg_string(vm, args, 0)?),
            )),
            "find" | "findnocase" => {
                let needle = builder_arg_string(vm, args, 0)?;
                let (haystack, needle) = if name == "findnocase" {
                    (self.value.to_lowercase(), needle.to_lowercase())
                } else {
                    (self.value.clone(), needle)
                };
                let start = args.get(1).map(BxValue::as_number).unwrap_or(1.0).max(1.0) as usize;
                let offset = char_to_byte_offset(&haystack, start - 1);
                let found = haystack[offset..]
                    .find(&needle)
                    .map(|idx| byte_to_char_count(&haystack[..offset + idx]) + 1)
                    .unwrap_or(0);
                Ok(BxValue::new_number(found as f64))
            }
            "delete" => {
                self.value = builder_replace_range(
                    &self.value,
                    builder_position(args, 0)?,
                    builder_position(args, 1)?,
                    "",
                );
                Ok(self_value())
            }
            "insert" => {
                self.value = builder_insert_at(
                    &self.value,
                    builder_position(args, 0)?,
                    &builder_arg_string(vm, args, 1)?,
                );
                Ok(self_value())
            }
            "replace" => {
                self.value = builder_replace_range(
                    &self.value,
                    builder_position(args, 0)?,
                    builder_position(args, 1)?,
                    &builder_arg_string(vm, args, 2)?,
                );
                Ok(self_value())
            }
            "reverse" => {
                self.value = self.value.chars().rev().collect();
                Ok(self_value())
            }
            "trim" => {
                self.value = self.value.trim().to_string();
                Ok(self_value())
            }
            "left" => Ok(BxValue::new_ptr(
                vm.string_new(builder_left(&self.value, builder_position(args, 0)?)),
            )),
            "right" => Ok(BxValue::new_ptr(
                vm.string_new(builder_right(&self.value, builder_position(args, 0)?)),
            )),
            "mid" => {
                let start = builder_position(args, 0)?;
                let count = args
                    .get(1)
                    .map(BxValue::as_number)
                    .map(|value| value as usize);
                Ok(BxValue::new_ptr(vm.string_new(builder_mid(
                    &self.value,
                    start,
                    count,
                ))))
            }
            _ => Err(format!("StringBuilder method '{}' not found", name)),
        }
    }

    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

fn builder_arg_string(vm: &mut dyn BxVM, args: &[BxValue], index: usize) -> Result<String, String> {
    args.get(index)
        .copied()
        .map(|value| vm.to_string(value))
        .ok_or_else(|| "StringBuilder method is missing an argument".to_string())
}

fn builder_position(args: &[BxValue], index: usize) -> Result<isize, String> {
    args.get(index)
        .map(|value| value.as_number() as isize)
        .ok_or_else(|| "StringBuilder method is missing a position".to_string())
}

fn char_to_byte_offset(value: &str, index: usize) -> usize {
    value
        .char_indices()
        .nth(index)
        .map_or(value.len(), |(offset, _)| offset)
}

fn byte_to_char_count(value: &str) -> usize {
    value.chars().count()
}

fn builder_insert_at(value: &str, position: isize, text: &str) -> String {
    let position = position.max(1) as usize;
    let offset = char_to_byte_offset(value, position - 1);
    format!("{}{}{}", &value[..offset], text, &value[offset..])
}

fn builder_replace_range(value: &str, start: isize, end: isize, replacement: &str) -> String {
    let start = start.max(1) as usize;
    let end = end.max(start as isize) as usize;
    let start_offset = char_to_byte_offset(value, start - 1);
    let end_offset = char_to_byte_offset(value, end);
    format!(
        "{}{}{}",
        &value[..start_offset],
        replacement,
        &value[end_offset..]
    )
}

fn builder_left(value: &str, count: isize) -> String {
    let length = value.chars().count() as isize;
    let count = if count < 0 { length + count } else { count }.clamp(0, length) as usize;
    value[..char_to_byte_offset(value, count)].to_string()
}

fn builder_right(value: &str, count: isize) -> String {
    let length = value.chars().count() as isize;
    let count = if count < 0 { length + count } else { count }.clamp(0, length) as usize;
    value[char_to_byte_offset(value, length as usize - count)..].to_string()
}

fn builder_mid(value: &str, start: isize, count: Option<usize>) -> String {
    let start = start.max(1) as usize;
    let start_offset = char_to_byte_offset(value, start - 1);
    let end = count.map_or(value.chars().count(), |count| start - 1 + count);
    let end_offset = char_to_byte_offset(value, end);
    value[start_offset..end_offset].to_string()
}

fn string_builder_new_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let value = args
        .first()
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .unwrap_or_default();
    let id = vm.native_object_new(Rc::new(RefCell::new(StringBuilderObject { value })));
    Ok(BxValue::new_ptr(id))
}

fn is_string_builder_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let Some(id) = args.first().and_then(BxValue::as_gc_id) else {
        return Ok(BxValue::new_bool(false));
    };
    Ok(BxValue::new_bool(
        vm.native_object_call_method(id, "__is_string_builder", &[])
            .is_ok(),
    ))
}

fn string_builder_call(
    vm: &mut dyn BxVM,
    args: &[BxValue],
    method: &str,
) -> Result<BxValue, String> {
    let id = args
        .first()
        .and_then(BxValue::as_gc_id)
        .ok_or("Expected a StringBuilder")?;
    vm.native_object_call_method(id, method, &args[1..])
}

macro_rules! string_builder_bif {
    ($name:ident, $method:literal) => {
        fn $name(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
            string_builder_call(vm, args, $method)
        }
    };
}

string_builder_bif!(string_builder_append_bif, "append");
string_builder_bif!(string_builder_clear_bif, "clear");
string_builder_bif!(string_builder_contains_bif, "contains");
string_builder_bif!(string_builder_delete_bif, "delete");
string_builder_bif!(string_builder_ends_with_bif, "endsWith");
string_builder_bif!(string_builder_find_bif, "find");
string_builder_bif!(string_builder_insert_bif, "insert");
string_builder_bif!(string_builder_left_bif, "left");
string_builder_bif!(string_builder_mid_bif, "mid");
string_builder_bif!(string_builder_prepend_bif, "prepend");
string_builder_bif!(string_builder_replace_bif, "replace");
string_builder_bif!(string_builder_reverse_bif, "reverse");
string_builder_bif!(string_builder_right_bif, "right");
string_builder_bif!(string_builder_starts_with_bif, "startsWith");
string_builder_bif!(string_builder_trim_bif, "trim");

fn future_on_error(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("onError() expects 2 arguments: (future, callback)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.future_on_error(id, args[1]);
        Ok(args[0])
    } else {
        Err("First argument to onError must be a future".to_string())
    }
}

fn ucase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("ucase() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]).to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn lcase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("lcase() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]).to_lowercase();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn trim_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("trim() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]).trim().to_string();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn ltrim_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args.len() > 2 {
        return Err("ltrim() expects 1 or 2 arguments: (string, chars?)".to_string());
    }
    let input = vm.to_string(args[0]);
    if input.is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }
    let result = if args.len() == 2 {
        let chars = vm.to_string(args[1]);
        let chars_vec: Vec<char> = chars.chars().collect();
        let start = input
            .char_indices()
            .find(|(_, c)| !chars_vec.contains(c))
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        input[start..].to_string()
    } else {
        input.trim_start().to_string()
    };
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn rtrim_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args.len() > 2 {
        return Err("rtrim() expects 1 or 2 arguments: (string, chars?)".to_string());
    }
    let input = vm.to_string(args[0]);
    if input.is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }
    let result = if args.len() == 2 {
        let chars = vm.to_string(args[1]);
        let chars_vec: Vec<char> = chars.chars().collect();
        let end = input
            .char_indices()
            .rev()
            .find(|(_, c)| !chars_vec.contains(c))
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        input[..end].to_string()
    } else {
        input.trim_end().to_string()
    };
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn compare_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("compare() expects exactly 2 arguments: (string1, string2)".to_string());
    }
    let s1 = vm.to_string(args[0]);
    let s2 = vm.to_string(args[1]);
    let result = match s1.cmp(&s2) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    };
    Ok(BxValue::new_number(result))
}

fn compare_no_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("compareNoCase() expects exactly 2 arguments: (string1, string2)".to_string());
    }
    let s1 = vm.to_string(args[0]).to_lowercase();
    let s2 = vm.to_string(args[1]).to_lowercase();
    let result = match s1.cmp(&s2) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    };
    Ok(BxValue::new_number(result))
}

fn remove_chars_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 3 {
        return Err(
            "removeChars() expects exactly 3 arguments: (string, start, count)".to_string(),
        );
    }
    let input = vm.to_string(args[0]);
    let start = args[1].as_number() as usize;
    let count = args[2].as_number() as usize;
    if start < 1 {
        return Err("removeChars() start position must be >= 1".to_string());
    }
    let chars: Vec<char> = input.chars().collect();
    let start_idx = start - 1;
    let mut result = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i < start_idx || i >= start_idx + count {
            result.push(*c);
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn strip_cr_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("stripCR() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let result: String = input.chars().filter(|&c| c != '\r').collect();
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn uc_first_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args.len() > 3 {
        return Err(
            "ucFirst() expects 1 to 3 arguments: (string, doAll?, doLowerIfAllUppercase?)"
                .to_string(),
        );
    }
    let input = vm.to_string(args[0]);
    if input.is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }
    let do_all = if args.len() > 1 {
        args[1].as_bool()
    } else {
        false
    };
    let do_lower = if args.len() > 2 {
        args[2].as_bool()
    } else {
        false
    };
    let words: Vec<&str> = input.split_whitespace().collect();
    let mut result_parts: Vec<String> = Vec::new();
    for word in words {
        let mut w = word.to_string();
        if do_lower && w.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
            w = w.to_lowercase();
        }
        if do_all || result_parts.is_empty() {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                w = first.to_uppercase().to_string() + chars.as_str();
            }
        }
        result_parts.push(w);
    }
    let result = result_parts.join(" ");
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn replace_no_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("replaceNoCase() expects at least 3 arguments: (string, substring, replacement, scope?)".to_string());
    }
    let input = vm.to_string(args[0]);
    let search = vm.to_string(args[1]);
    let replacement = vm.to_string(args[2]);
    let scope = if args.len() > 3 {
        vm.to_string(args[3]).to_lowercase()
    } else {
        "one".to_string()
    };
    let input_lower = input.to_lowercase();
    let search_lower = search.to_lowercase();
    let result = if scope == "all" {
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = input.chars().collect();
        let chars_lower: Vec<char> = input_lower.chars().collect();
        let search_chars: Vec<char> = search_lower.chars().collect();
        let search_len = search_chars.len();
        while i < chars.len() {
            if i + search_len <= chars.len() && chars_lower[i..i + search_len] == search_chars[..] {
                result.push_str(&replacement);
                i += search_len;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result
    } else {
        if let Some(pos) = input_lower.find(&search_lower) {
            format!(
                "{}{}{}",
                &input[..pos],
                replacement,
                &input[pos + search.len()..]
            )
        } else {
            input
        }
    };
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn string_ends_with_no_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err(
            "stringEndsWithNoCase() expects exactly 2 arguments: (string, suffix)".to_string(),
        );
    }
    let input = vm.to_string(args[0]).to_lowercase();
    let suffix = vm.to_string(args[1]).to_lowercase();
    Ok(BxValue::new_bool(input.ends_with(&suffix)))
}

fn string_starts_with_no_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err(
            "stringStartsWithNoCase() expects exactly 2 arguments: (string, prefix)".to_string(),
        );
    }
    let input = vm.to_string(args[0]).to_lowercase();
    let prefix = vm.to_string(args[1]).to_lowercase();
    Ok(BxValue::new_bool(input.starts_with(&prefix)))
}

fn ascii_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("ascii() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    if input.is_empty() {
        return Err("ascii() requires a non-empty string".to_string());
    }
    let first_char = input.chars().next().unwrap();
    Ok(BxValue::new_number(first_char as u32 as f64))
}

fn find_one_of_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("findOneOf() expects at least 2 arguments: (set, string, start?)".to_string());
    }
    let set = vm.to_string(args[0]);
    let input = vm.to_string(args[1]);
    let start = if args.len() > 2 {
        args[2].as_number() as usize
    } else {
        1
    };
    let start = start.max(1);
    let start_idx = start - 1;
    let chars: Vec<char> = input.chars().collect();
    let set_chars: Vec<char> = set.chars().collect();
    for i in start_idx..chars.len() {
        if set_chars.contains(&chars[i]) {
            return Ok(BxValue::new_number((i + 1) as f64));
        }
    }
    Ok(BxValue::new_number(0.0))
}

fn insert_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 3 {
        return Err(
            "insert() expects exactly 3 arguments: (substring, string, position)".to_string(),
        );
    }
    let substring = vm.to_string(args[0]);
    let input = vm.to_string(args[1]);
    let position = args[2].as_number() as usize;
    let chars: Vec<char> = input.chars().collect();
    if position > chars.len() {
        return Err(format!("insert() position {} is out of range", position));
    }
    let result = format!(
        "{}{}{}",
        chars[..position].iter().collect::<String>(),
        substring,
        chars[position..].iter().collect::<String>()
    );
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn re_escape_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("reEscape() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let special_chars = "\\.^$|()[]{}*+?";
    let mut result = String::new();
    for c in input.chars() {
        if special_chars.contains(c) {
            result.push('\\');
        }
        result.push(c);
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn yes_no_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("yesNoFormat() expects exactly 1 argument".to_string());
    }
    let val = args[0];
    let is_true = if val.is_bool() {
        val.as_bool()
    } else if val.is_number() {
        val.as_number() != 0.0
    } else if val.is_int() {
        val.as_int() != 0
    } else if val.is_null() {
        false
    } else if let Some(id) = val.as_gc_id() {
        let s = vm.to_string(val);
        !s.is_empty() && s.to_lowercase() != "false"
    } else {
        false
    };
    Ok(BxValue::new_ptr(vm.string_new(
        if is_true { "Yes" } else { "No" }.to_string(),
    )))
}

fn snake_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("snakeCase() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let result = to_case(&input, '_');
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn kebab_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("kebabCase() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let result = to_case(&input, '-');
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn camel_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("camelCase() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let words = split_into_words(&input);
    let mut result = String::new();
    for (i, word) in words.iter().enumerate() {
        if i == 0 {
            result.push_str(&word.to_lowercase());
        } else {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                result.push(first.to_uppercase().next().unwrap());
                result.push_str(&chars.as_str().to_lowercase());
            }
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn pascal_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("pascalCase() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let words = split_into_words(&input);
    let mut result = String::new();
    for word in words {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.push(first.to_uppercase().next().unwrap());
            result.push_str(&chars.as_str().to_lowercase());
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn split_into_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_was_lower = false;
    for c in input.chars() {
        if c == '_' || c == '-' || c == ' ' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            prev_was_lower = false;
        } else if c.is_uppercase() {
            if prev_was_lower && !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            current.push(c);
            prev_was_lower = false;
        } else {
            current.push(c);
            prev_was_lower = c.is_lowercase();
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn to_case(input: &str, separator: char) -> String {
    let words = split_into_words(input);
    words
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(&separator.to_string())
}

fn replace_list_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err(
            "replaceList() expects at least 3 arguments: (string, list1, list2)".to_string(),
        );
    }
    let mut input = vm.to_string(args[0]);
    let list1 = vm.to_string(args[1]);
    let list2 = vm.to_string(args[2]);
    let items1: Vec<&str> = list1.split(',').collect();
    let items2: Vec<&str> = list2.split(',').collect();
    if items2.len() < items1.len() {
        let result = input
            .split(',')
            .filter_map(|item| {
                items1
                    .iter()
                    .position(|search| *search == item)
                    .map_or(Some(item), |index| items2.get(index).copied())
            })
            .collect::<Vec<_>>()
            .join(",");
        return Ok(BxValue::new_ptr(vm.string_new(result)));
    }
    for (i, search) in items1.iter().enumerate() {
        input = input.replace(search, items2[i]);
    }
    Ok(BxValue::new_ptr(vm.string_new(input)))
}

fn repeat_string(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("repeatString() expects exactly 2 arguments".to_string());
    }
    let s = vm.to_string(args[0]);
    let count = args[1].as_number() as usize;
    let result = s.repeat(count);
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn find_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    find_bif_internal(vm, args, false, true)
}

fn find_no_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    find_bif_internal(vm, args, true, true)
}

fn string_find_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    find_bif_internal(vm, args, false, false)
}

fn string_find_no_case_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    find_bif_internal(vm, args, true, false)
}

fn string_ends_with_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("stringEndsWith() expects exactly 2 arguments: (string, suffix)".to_string());
    }
    let input = vm.to_string(args[0]);
    let suffix = vm.to_string(args[1]);
    Ok(BxValue::new_bool(input.ends_with(&suffix)))
}

fn string_starts_with_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("stringStartsWith() expects exactly 2 arguments: (string, prefix)".to_string());
    }
    let input = vm.to_string(args[0]);
    let prefix = vm.to_string(args[1]);
    Ok(BxValue::new_bool(input.starts_with(&prefix)))
}

fn val_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("val() expects exactly 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);

    if input.is_empty() {
        return Ok(BxValue::new_number(0.0));
    }

    let mut result = String::new();
    let mut found_dot = false;
    let mut found_digit = false;

    for (i, c) in input.chars().enumerate() {
        // Only allow digits, one dot, and minus at the start
        if c.is_ascii_digit() {
            found_digit = true;
            result.push(c);
        } else if c == '.' && !found_dot {
            found_dot = true;
            result.push(c);
        } else if c == '-' && i == 0 {
            result.push(c);
        } else {
            break;
        }
    }

    if !found_digit {
        return Ok(BxValue::new_number(0.0));
    }

    // Remove trailing dot if present
    if result.ends_with('.') {
        result.pop();
    }

    let num: f64 = result.parse().unwrap_or(0.0);
    Ok(BxValue::new_number(num))
}

fn find_bif_internal(
    vm: &mut dyn BxVM,
    args: &[BxValue],
    ignore_case: bool,
    substring_first: bool,
) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("find() expects at least 2 arguments: (substring, string)".to_string());
    }

    let (substring, input) = if substring_first {
        (vm.to_string(args[0]), vm.to_string(args[1]))
    } else {
        (vm.to_string(args[1]), vm.to_string(args[0]))
    };
    let start = if args.len() > 2 {
        args[2].as_number() as isize
    } else {
        1
    };
    let start = start.max(1) as usize;
    let start_idx = start.saturating_sub(1);
    let input_chars = input.chars().count();
    let start_byte_idx = if start_idx >= input_chars {
        input.len()
    } else {
        input
            .char_indices()
            .nth(start_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(input.len())
    };
    let haystack = &input[start_byte_idx..];

    let position = if ignore_case {
        let needle = substring.to_lowercase();
        let hay = haystack.to_lowercase();
        hay.find(&needle).map(|idx| idx + start_idx + 1)
    } else {
        haystack.find(&substring).map(|idx| idx + start_idx + 1)
    };

    Ok(BxValue::new_number(position.unwrap_or(0) as f64))
}

fn left_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("left() expects 2 arguments: (string, count)".to_string());
    }
    let input = vm.to_string(args[0]);
    if input.is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }

    let count = args[1].as_number() as isize;
    if count == 0 {
        return Err("Count cannot be zero".to_string());
    }

    let chars: Vec<char> = input.chars().collect();
    let len = chars.len() as isize;
    let end = if count > 0 {
        count.min(len)
    } else {
        (len + count).max(0)
    } as usize;

    Ok(BxValue::new_ptr(
        vm.string_new(chars[..end].iter().collect()),
    ))
}

fn right_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("right() expects 2 arguments: (string, count)".to_string());
    }
    let input = vm.to_string(args[0]);
    if input.is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }

    let count = args[1].as_number() as isize;
    if count == 0 {
        return Err("Count cannot be zero.".to_string());
    }

    let chars: Vec<char> = input.chars().collect();
    let len = chars.len() as isize;
    let start = if count > 0 {
        (len - count.max(0)).max(0)
    } else {
        (-count).min(len)
    } as usize;

    Ok(BxValue::new_ptr(
        vm.string_new(chars[start..].iter().collect()),
    ))
}

fn reverse_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("reverse() expects exactly 1 argument".to_string());
    }

    let reversed = vm.to_string(args[0]).chars().rev().collect::<String>();
    Ok(BxValue::new_ptr(vm.string_new(reversed)))
}

fn span_excluding_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("spanExcluding() expects 2 arguments: (string, set)".to_string());
    }

    let input = vm.to_string(args[0]);
    let set = vm.to_string(args[1]);
    if input.is_empty() {
        return Err("spanExcluding() expects a non-empty string".to_string());
    }

    let end = input
        .chars()
        .position(|ch| set.chars().any(|needle| needle == ch))
        .unwrap_or_else(|| input.chars().count());
    Ok(BxValue::new_ptr(
        vm.string_new(input.chars().take(end).collect()),
    ))
}

fn span_including_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("spanIncluding() expects 2 arguments: (string, set)".to_string());
    }

    let input = vm.to_string(args[0]);
    let set = vm.to_string(args[1]);
    if input.is_empty() {
        return Err("spanIncluding() expects a non-empty string".to_string());
    }

    let end = input
        .chars()
        .position(|ch| !set.chars().any(|needle| needle == ch))
        .unwrap_or_else(|| input.chars().count());
    Ok(BxValue::new_ptr(
        vm.string_new(input.chars().take(end).collect()),
    ))
}

fn to_string_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("toString() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]);
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn cast_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("cast() expects exactly 2 arguments: (value, type)".to_string());
    }

    let value = args[0];
    let type_name = vm.to_string(args[1]);
    match type_name.trim().to_ascii_lowercase().as_str() {
        "string" if value.is_null() => Err("Could not cast null to type [string]".to_string()),
        "numeric" | "number" if value.is_null() => Ok(BxValue::new_number(0.0)),
        "numeric" | "number" if value.is_bool() => Err(format!(
            "Could not cast object [{}] to type [{}]",
            vm.to_string(value),
            type_name
        )),
        "double" if value.is_null() => Ok(BxValue::new_number(0.0)),
        _ => vm.cast_value_to_type(value, &type_name),
    }
}

fn list_to_array(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listToArray() expects at least 1 argument".to_string());
    }
    let parts = parse_list_items(
        &vm.to_string(args[0]),
        if args.len() > 1 {
            vm.to_string(args[1])
        } else {
            ",".to_string()
        },
        if args.len() > 2 {
            args[2].as_bool()
        } else {
            false
        },
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
    );
    let array_id = vm.array_new();
    for part in parts {
        let s_id = vm.string_new(part);
        vm.array_push(array_id, BxValue::new_ptr(s_id));
    }
    Ok(BxValue::new_ptr(array_id))
}

fn list_len(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listLen() expects at least 1 argument".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        if args.len() > 1 {
            vm.to_string(args[1])
        } else {
            ",".to_string()
        },
        if args.len() > 2 {
            args[2].as_bool()
        } else {
            false
        },
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
    );
    Ok(BxValue::new_number(items.len() as f64))
}

fn list_get_at(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listGetAt() expects at least 2 arguments: (list, position)".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        if args.len() > 2 {
            vm.to_string(args[2])
        } else {
            ",".to_string()
        },
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
        if args.len() > 4 {
            args[4].as_bool()
        } else {
            false
        },
    );
    let pos = args[1].as_number() as isize;
    if pos < 1 || pos as usize > items.len() {
        return Err(format!("listGetAt() position {} out of range", pos));
    }
    Ok(BxValue::new_ptr(
        vm.string_new(items[(pos - 1) as usize].clone()),
    ))
}

fn list_append(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listAppend() expects at least 2 arguments: (list, value)".to_string());
    }
    let delimiter = if args.len() > 2 {
        vm.to_string(args[2])
    } else {
        ",".to_string()
    };
    let multi = if args.len() > 4 {
        args[4].as_bool()
    } else {
        false
    };
    let mut items = parse_list_items(
        &vm.to_string(args[0]),
        delimiter.clone(),
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
        multi,
    );
    if !args[1].is_null() {
        items.push(vm.to_string(args[1]));
    }
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delimiter, multi)),
    ))
}

fn list_first(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listFirst() expects at least 1 argument".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        if args.len() > 1 {
            vm.to_string(args[1])
        } else {
            ",".to_string()
        },
        if args.len() > 2 {
            args[2].as_bool()
        } else {
            false
        },
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
    );
    let first = items.first().cloned().unwrap_or_default();
    Ok(BxValue::new_ptr(vm.string_new(first)))
}

fn list_last(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listLast() expects at least 1 argument".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        if args.len() > 1 {
            vm.to_string(args[1])
        } else {
            ",".to_string()
        },
        if args.len() > 2 {
            args[2].as_bool()
        } else {
            false
        },
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
    );
    let last = items.last().cloned().unwrap_or_default();
    Ok(BxValue::new_ptr(vm.string_new(last)))
}

fn list_rest(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listRest() expects at least 1 argument".to_string());
    }
    let delimiter = if args.len() > 1 {
        vm.to_string(args[1])
    } else {
        ",".to_string()
    };
    let multi = if args.len() > 3 {
        args[3].as_bool()
    } else {
        false
    };
    let offset = if args.len() > 4 {
        args[4].as_number() as usize
    } else {
        1
    };
    let mut items = parse_list_items(
        &vm.to_string(args[0]),
        delimiter.clone(),
        if args.len() > 2 {
            args[2].as_bool()
        } else {
            false
        },
        multi,
    );
    let cutoff = offset.min(items.len());
    items.drain(0..cutoff);
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delimiter, multi)),
    ))
}

fn list_delete_at(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listDeleteAt() expects at least 2 arguments: (list, position)".to_string());
    }
    let delimiter = if args.len() > 2 {
        vm.to_string(args[2])
    } else {
        ",".to_string()
    };
    let multi = if args.len() > 4 {
        args[4].as_bool()
    } else {
        false
    };
    let mut items = parse_list_items(
        &vm.to_string(args[0]),
        delimiter.clone(),
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
        multi,
    );
    let pos = args[1].as_number() as isize;
    if pos < 1 || pos as usize > items.len() {
        return Err(format!("listDeleteAt() position {} out of range", pos));
    }
    items.remove((pos - 1) as usize);
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delimiter, multi)),
    ))
}

fn list_find(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    list_find_internal(vm, args, false)
}

fn list_find_no_case(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    list_find_internal(vm, args, true)
}

fn list_find_internal(
    vm: &mut dyn BxVM,
    args: &[BxValue],
    no_case: bool,
) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listFind() expects at least 2 arguments: (list, value)".to_string());
    }

    let value = if no_case {
        vm.to_string(args[1]).to_lowercase()
    } else {
        vm.to_string(args[1])
    };
    let items = parse_list_items(
        &vm.to_string(args[0]),
        if args.len() > 2 {
            vm.to_string(args[2])
        } else {
            ",".to_string()
        },
        if args.len() > 3 {
            args[3].as_bool()
        } else {
            false
        },
        if args.len() > 4 {
            args[4].as_bool()
        } else {
            false
        },
    );

    for (idx, part) in items.iter().enumerate() {
        let candidate = if no_case {
            part.to_lowercase()
        } else {
            part.clone()
        };
        if candidate == value {
            return Ok(BxValue::new_number((idx + 1) as f64));
        }
    }
    Ok(BxValue::new_number(0.0))
}

fn list_sort(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listSort() expects at least 1 argument".to_string());
    }
    let delimiter = if args.len() > 3 {
        vm.to_string(args[3])
    } else {
        ",".to_string()
    };
    let multi = if args.len() > 5 {
        args[5].as_bool()
    } else {
        false
    };
    let mut items = parse_list_items(
        &vm.to_string(args[0]),
        delimiter.clone(),
        if args.len() > 4 {
            args[4].as_bool()
        } else {
            false
        },
        multi,
    );
    let sort_type = if args.len() > 1 {
        vm.to_string(args[1]).to_lowercase()
    } else {
        "text".to_string()
    };
    let sort_order = if args.len() > 2 {
        vm.to_string(args[2]).to_lowercase()
    } else {
        "asc".to_string()
    };

    items.sort_by(|a, b| {
        let ord = match sort_type.as_str() {
            "numeric" => {
                let na = a.trim().parse::<f64>().unwrap_or(0.0);
                let nb = b.trim().parse::<f64>().unwrap_or(0.0);
                na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
            }
            "textnocase" => a.to_lowercase().cmp(&b.to_lowercase()),
            _ => a.cmp(b),
        };
        if sort_order == "desc" {
            ord.reverse()
        } else {
            ord
        }
    });

    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delimiter, multi)),
    ))
}

pub(super) fn parse_list_items(
    list: &str,
    delimiter: String,
    include_empty: bool,
    multi: bool,
) -> Vec<String> {
    if list.is_empty() {
        return Vec::new();
    }
    if delimiter.is_empty() {
        return list.chars().map(|ch| ch.to_string()).collect();
    }

    let mut items = Vec::new();
    if multi {
        for part in list.split(&delimiter) {
            if include_empty || !part.is_empty() {
                items.push(part.to_string());
            }
        }
    } else {
        let delims: Vec<char> = delimiter.chars().collect();
        let mut current = String::new();
        for ch in list.chars() {
            if delims.contains(&ch) {
                if include_empty || !current.is_empty() {
                    items.push(current.clone());
                }
                current.clear();
            } else {
                current.push(ch);
            }
        }
        if include_empty || !current.is_empty() {
            items.push(current);
        }
    }
    items
}

pub(super) fn join_list(items: &[String], delimiter: &str, multi: bool) -> String {
    if items.is_empty() {
        return String::new();
    }
    if multi {
        items.join(delimiter)
    } else {
        let join_delimiter = delimiter.chars().next().unwrap_or(',');
        items.join(&join_delimiter.to_string())
    }
}

fn index_of(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("indexOf() expects 2 arguments: (string, substring)".to_string());
    }
    let s = vm.to_string(args[0]);
    let sub = vm.to_string(args[1]);

    match s.find(&sub) {
        Some(idx) => Ok(BxValue::new_number(idx as f64 + 1.0)), // 1-based index for BoxLang consistency
        None => Ok(BxValue::new_number(-1.0)),
    }
}

fn chr_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("chr() expects 1 argument".to_string());
    }
    let code = args[0].as_number();
    if code > 0xffff as f64 {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }
    if code < 0.0 {
        return Err(format!("Invalid character code: {}", code));
    }
    let code = code as u32;
    let c = std::char::from_u32(code).ok_or_else(|| format!("Invalid character code: {}", code))?;
    let s_id = vm.string_new(c.to_string());
    Ok(BxValue::new_ptr(s_id))
}

fn re_match(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    regex_match(vm, args, false)
}

fn re_match_no_case(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    regex_match(vm, args, true)
}

fn re_find(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    regex_find(vm, args, false)
}

fn re_find_no_case(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    regex_find(vm, args, true)
}

fn re_replace(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    regex_replace(vm, args, false)
}

fn re_replace_no_case(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    regex_replace(vm, args, true)
}

fn regex_prepare(pattern: &str, no_case: bool) -> String {
    let pattern = regex_posix_replace(pattern, no_case);
    regex_replace_non_quantifier_curly_braces(&pattern)
}

fn regex_match(vm: &mut dyn BxVM, args: &[BxValue], no_case: bool) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("reMatch() expects 2 arguments: (regex, string)".to_string());
    }
    let pattern = regex_prepare(&vm.to_string(args[0]), no_case);
    let text = vm.to_string(args[1]);
    let regex = regex::RegexBuilder::new(&pattern)
        .case_insensitive(no_case)
        .dot_matches_new_line(true)
        .build()
        .map_err(|e| format!("Invalid regex: {}", e))?;
    let array_id = vm.array_new();
    for cap in regex.find_iter(&text) {
        let s_id = vm.string_new(cap.as_str().to_string());
        vm.array_push(array_id, BxValue::new_ptr(s_id));
    }
    Ok(BxValue::new_ptr(array_id))
}

fn regex_find(vm: &mut dyn BxVM, args: &[BxValue], no_case: bool) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("reFind() expects at least 2 arguments: (regex, string)".to_string());
    }

    let pattern = regex_prepare(&vm.to_string(args[0]), no_case);
    let text = vm.to_string(args[1]);
    let mut start = if args.len() > 2 {
        args[2].as_number() as isize
    } else {
        1
    };
    let return_subs = if args.len() > 3 {
        args[3].as_bool()
    } else {
        false
    };
    let scope = if args.len() > 4 {
        vm.to_string(args[4]).to_lowercase()
    } else {
        "one".to_string()
    };
    if start < 1 {
        start = 1;
    }
    let start_idx = (start - 1) as usize;
    let start_byte_idx = char_to_byte_index(&text, start_idx);
    let regex = regex::RegexBuilder::new(&pattern)
        .case_insensitive(no_case)
        .dot_matches_new_line(true)
        .build()
        .map_err(|e| format!("Invalid regex: {}", e))?;
    let region = &text[start_byte_idx..];
    let mut matches = Vec::new();
    for cap in regex.captures_iter(region) {
        let Some(m) = cap.get(0) else { continue };
        let mut len_array = vec![BxValue::new_number(m.as_str().chars().count() as f64)];
        let mut match_array = vec![BxValue::new_ptr(vm.string_new(m.as_str().to_string()))];
        let mut pos_array = vec![BxValue::new_number(
            char_count_to_pos(region, m.start()) as f64 + start_idx as f64,
        )];
        for idx in 1..=cap.len().saturating_sub(1) {
            match cap.get(idx) {
                Some(group) => {
                    len_array.push(BxValue::new_number(group.as_str().chars().count() as f64));
                    match_array.push(BxValue::new_ptr(vm.string_new(group.as_str().to_string())));
                    pos_array.push(BxValue::new_number(
                        char_count_to_pos(region, group.start()) as f64 + start_idx as f64,
                    ));
                }
                None => {
                    len_array.push(BxValue::new_number(0.0));
                    match_array.push(BxValue::new_ptr(vm.string_new(String::new())));
                    pos_array.push(BxValue::new_number(0.0));
                }
            }
        }
        let len_id = vm.array_new();
        for item in len_array {
            vm.array_push(len_id, item);
        }
        let match_id = vm.array_new();
        for item in match_array {
            vm.array_push(match_id, item);
        }
        let pos_id = vm.array_new();
        for item in pos_array {
            vm.array_push(pos_id, item);
        }
        let struct_id = vm.struct_new();
        vm.struct_set(struct_id, "len", BxValue::new_ptr(len_id));
        vm.struct_set(struct_id, "match", BxValue::new_ptr(match_id));
        vm.struct_set(struct_id, "pos", BxValue::new_ptr(pos_id));
        matches.push(BxValue::new_ptr(struct_id));
        if scope == "one" {
            break;
        }
    }
    if return_subs {
        if matches.is_empty() {
            let struct_id = vm.struct_new();
            let len_id = vm.array_new();
            vm.array_push(len_id, BxValue::new_number(0.0));
            let match_id = vm.array_new();
            let empty_id = vm.string_new(String::new());
            vm.array_push(match_id, BxValue::new_ptr(empty_id));
            let pos_id = vm.array_new();
            vm.array_push(pos_id, BxValue::new_number(0.0));
            vm.struct_set(struct_id, "len", BxValue::new_ptr(len_id));
            vm.struct_set(struct_id, "match", BxValue::new_ptr(match_id));
            vm.struct_set(struct_id, "pos", BxValue::new_ptr(pos_id));
            if scope == "all" {
                let arr_id = vm.array_new();
                vm.array_push(arr_id, BxValue::new_ptr(struct_id));
                Ok(BxValue::new_ptr(arr_id))
            } else {
                Ok(BxValue::new_ptr(struct_id))
            }
        } else if scope == "all" {
            let arr_id = vm.array_new();
            for m in matches {
                vm.array_push(arr_id, m);
            }
            Ok(BxValue::new_ptr(arr_id))
        } else {
            Ok(matches.remove(0))
        }
    } else {
        if matches.is_empty() {
            Ok(BxValue::new_number(0.0))
        } else if scope == "all" {
            let arr_id = vm.array_new();
            for m in matches {
                if let Some(struct_id) = m.as_gc_id() {
                    let pos_val = vm.struct_get(struct_id, "pos");
                    if let Some(pos_arr_id) = pos_val.as_gc_id() {
                        let first = vm.array_get(pos_arr_id, 0);
                        vm.array_push(arr_id, first);
                    }
                }
            }
            Ok(BxValue::new_ptr(arr_id))
        } else {
            if let Some(struct_id) = matches[0].as_gc_id() {
                let pos_val = vm.struct_get(struct_id, "pos");
                if let Some(pos_arr_id) = pos_val.as_gc_id() {
                    Ok(vm.array_get(pos_arr_id, 0))
                } else {
                    Ok(BxValue::new_number(0.0))
                }
            } else {
                Ok(BxValue::new_number(0.0))
            }
        }
    }
}

fn regex_replace(vm: &mut dyn BxVM, args: &[BxValue], no_case: bool) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err(
            "reReplace() expects at least 3 arguments: (string, regex, replacement)".to_string(),
        );
    }
    let string = vm.to_string(args[0]);
    let pattern = regex_prepare(&vm.to_string(args[1]), no_case);
    let substring = vm.to_string(args[2]);
    let scope = if args.len() > 3 {
        vm.to_string(args[3]).to_lowercase()
    } else {
        "one".to_string()
    };

    let regex = regex::RegexBuilder::new(&pattern)
        .case_insensitive(no_case)
        .dot_matches_new_line(true)
        .build()
        .map_err(|e| format!("Invalid regex: {}", e))?;

    let result = if scope == "all" {
        regex.replace_all(&string, substring.as_str()).to_string()
    } else {
        regex.replace(&string, substring.as_str()).to_string()
    };
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn char_to_byte_index(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn char_count_to_pos(text: &str, byte_idx: usize) -> usize {
    text[..byte_idx].chars().count() + 1
}

fn regex_posix_replace(expression: &str, no_case: bool) -> String {
    let mut return_expression = expression.to_string();
    let replacements = [
        ("[:alnum:]", "a-zA-Z0-9"),
        ("[:alpha:]", "a-zA-Z"),
        ("[:blank:]", " \\t"),
        ("[:cntrl:]", "\\x00-\\x1F\\x7F"),
        ("[:digit:]", "0-9"),
        ("[:graph:]", "\\x21-\\x7E"),
        ("[:lower:]", if no_case { "a-zA-Z" } else { "a-z" }),
        ("[:print:]", "\\x20-\\x7E"),
        ("[:punct:]", "!\"#$%&'()*+,-./:;<=>?@\\[\\]^_`{|}~"),
        ("[:space:]", "\\s"),
        ("[:upper:]", if no_case { "a-zA-Z" } else { "A-Z" }),
        ("[:xdigit:]", "0-9a-fA-F"),
    ];
    for (needle, replacement) in replacements {
        return_expression = return_expression.replace(needle, replacement);
        return_expression =
            return_expression.replace(&format!("[{}]", needle), &format!("[{}]", replacement));
    }
    return_expression
}

fn regex_replace_non_quantifier_curly_braces(input: &str) -> String {
    let mut escaped = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            if i + 1 < chars.len() {
                escaped.push(c);
                escaped.push(chars[i + 1]);
                i += 2;
                continue;
            }
        }
        if c == '{' {
            let mut j = i + 1;
            let mut has_digit = false;
            while j < chars.len() && chars[j].is_ascii_digit() {
                has_digit = true;
                j += 1;
            }
            if has_digit {
                if j < chars.len() && chars[j] == ',' {
                    j += 1;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                if j < chars.len() && chars[j] == '}' {
                    escaped.extend(chars[i..=j].iter());
                    i = j + 1;
                    continue;
                }
            }
            escaped.push_str("\\{");
        } else if c == '}' {
            escaped.push_str("\\}");
        } else {
            escaped.push(c);
        }
        i += 1;
    }
    escaped
}

fn mid_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("mid() expects at least 2 arguments: (string, start)".to_string());
    }
    let s = vm.to_string(args[0]);
    let start = args[1].as_number() as usize;
    if start == 0 {
        return Err("mid() start index must be 1-based".to_string());
    }
    let count = if args.len() > 2 {
        args[2].as_number() as usize
    } else {
        s.len()
    };
    let start_idx = start.saturating_sub(1);
    let result = s.chars().skip(start_idx).take(count).collect::<String>();
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn replace_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err(
            "replace() expects at least 3 arguments: (string, substring1, substring2)".to_string(),
        );
    }
    let s = vm.to_string(args[0]);
    let old = vm.to_string(args[1]);
    let new = vm.to_string(args[2]);
    let scope = if args.len() > 3 {
        vm.to_string(args[3]).to_lowercase()
    } else {
        "one".to_string()
    };
    let result = if scope == "all" {
        s.replace(&old, &new)
    } else {
        s.replacen(&old, &new, 1)
    };
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn round(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args.len() > 2 {
        return Err("round() expects 1 or 2 arguments".to_string());
    }
    if args[0].is_number() {
        let precision = args.get(1).map_or(0, |value| value.as_number() as i32);
        let factor = 10_f64.powi(precision);
        Ok(BxValue::new_number(
            (args[0].as_number() * factor).round() / factor,
        ))
    } else {
        Err("round() expects a number".to_string())
    }
}

fn floor_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("floor() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().floor()))
    } else {
        Err("floor() expects a number".to_string())
    }
}

fn int_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("int() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().trunc()))
    } else {
        Err("int() expects a number".to_string())
    }
}

fn ceiling_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("ceiling() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().ceil()))
    } else {
        Err("ceiling() expects a number".to_string())
    }
}

fn abs_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("abs() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().abs()))
    } else {
        Err("abs() expects a number".to_string())
    }
}

fn min_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("min() expects exactly 2 arguments".to_string());
    }
    if args[0].is_number() && args[1].is_number() {
        Ok(BxValue::new_number(
            args[0].as_number().min(args[1].as_number()),
        ))
    } else {
        Err("min() expects numbers".to_string())
    }
}

fn max_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("max() expects exactly 2 arguments".to_string());
    }
    if args[0].is_number() && args[1].is_number() {
        Ok(BxValue::new_number(
            args[0].as_number().max(args[1].as_number()),
        ))
    } else {
        Err("max() expects numbers".to_string())
    }
}

fn rand_range(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("randRange() expects exactly 2 arguments".to_string());
    }
    if !args[0].is_number() || !args[1].is_number() {
        return Err("randRange() expects numbers".to_string());
    }
    let id = math_rng(vm);
    vm.native_object_call_method(id, "randomrange", args)
}

fn rand(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if !args.is_empty() {
        return Err("rand() expects no arguments".to_string());
    }
    let id = math_rng(vm);
    vm.native_object_call_method(id, "random", &[])
}

fn randomize(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("randomize() expects at least 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        return Err(format!("randomize() expects a numeric seed, not {}", id));
    }
    if !args[0].is_number() {
        return Err("randomize() expects a number".to_string());
    }
    let id = math_rng(vm);
    vm.native_object_call_method(id, "seed", &[args[0]])
}

fn pi(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if !args.is_empty() {
        return Err("pi() expects no arguments".to_string());
    }
    Ok(BxValue::new_number(std::f64::consts::PI))
}

fn log_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args.len() > 2 {
        return Err("log() expects 1 or 2 arguments".to_string());
    }
    if !args[0].is_number() {
        return Err("log() expects a number".to_string());
    }
    let value = args[0].as_number();
    if args.len() == 1 {
        Ok(BxValue::new_number(value.ln()))
    } else if args[1].is_number() {
        Ok(BxValue::new_number(value.log(args[1].as_number())))
    } else {
        Err("log() expects numeric arguments".to_string())
    }
}

fn log10_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("log10() expects exactly 1 numeric argument".to_string());
    }
    Ok(BxValue::new_number(args[0].as_number().log10()))
}

fn exp_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("exp() expects exactly 1 numeric argument".to_string());
    }
    Ok(BxValue::new_number(args[0].as_number().exp()))
}

fn sin_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("sin() expects exactly 1 numeric argument".to_string());
    }
    Ok(BxValue::new_number(args[0].as_number().sin()))
}

fn cos_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("cos() expects exactly 1 numeric argument".to_string());
    }
    Ok(BxValue::new_number(args[0].as_number().cos()))
}

fn tan_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("tan() expects exactly 1 numeric argument".to_string());
    }
    Ok(BxValue::new_number(args[0].as_number().tan()))
}

fn asin_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("asin() expects exactly 1 numeric argument".to_string());
    }
    let value = args[0].as_number();
    if !(-1.0..=1.0).contains(&value) {
        return Err("asin() argument must be between -1 and 1".to_string());
    }
    Ok(BxValue::new_number(value.asin()))
}

fn acos_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("acos() expects exactly 1 numeric argument".to_string());
    }
    let value = args[0].as_number();
    if !(-1.0..=1.0).contains(&value) {
        return Err("acos() argument must be between -1 and 1".to_string());
    }
    Ok(BxValue::new_number(value.acos()))
}

fn atan_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 || !args[0].is_number() {
        return Err("atan() expects exactly 1 numeric argument".to_string());
    }
    Ok(BxValue::new_number(args[0].as_number().atan()))
}

fn atan2_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 || !args[0].is_number() || !args[1].is_number() {
        return Err("atan2() expects exactly 2 numeric arguments".to_string());
    }
    Ok(BxValue::new_number(
        args[0].as_number().atan2(args[1].as_number()),
    ))
}

fn len(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("len() expects exactly 1 argument".to_string());
    }
    if args[0].is_null() {
        return Ok(BxValue::new_number(0.0));
    }
    if args[0].is_number() || args[0].is_int() {
        return Ok(BxValue::new_number(
            vm.to_string(args[0]).chars().count() as f64
        ));
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_number(vm.get_len(id) as f64))
    } else {
        Err("len() expects a string, array, or struct".to_string())
    }
}

fn get_metadata_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("getMetadata() expects exactly 1 argument".to_string());
    }
    let metadata = vm.struct_new();
    let class_name = if vm.is_array_value(args[0]) {
        "array".to_string()
    } else if vm.is_struct_value(args[0]) {
        "struct".to_string()
    } else if vm.is_string_value(args[0]) {
        "string".to_string()
    } else if let Some(name) = vm.type_name_from_value(args[0]) {
        name
    } else if args[0].is_number() || args[0].is_int() {
        "numeric".to_string()
    } else {
        "object".to_string()
    };
    let class_value = BxValue::new_ptr(vm.string_new(class_name));
    vm.struct_set(metadata, "class", class_value);
    Ok(BxValue::new_ptr(metadata))
}

fn write_output_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    for arg in args {
        let s = vm.to_string(*arg);
        vm.write_output(&s);
    }
    Ok(BxValue::new_bool(true))
}

fn duplicate_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("duplicate() expects at least 1 argument".to_string());
    }
    let deep = args.get(1).map(|v| v.as_bool()).unwrap_or(true);
    let mut seen = HashMap::new();
    duplicate_value(vm, args[0], deep, &mut seen)
}

pub(super) fn duplicate_value(
    vm: &mut dyn BxVM,
    value: BxValue,
    deep: bool,
    seen: &mut HashMap<usize, BxValue>,
) -> Result<BxValue, String> {
    if value.is_null() || value.is_bool() || value.is_number() {
        return Ok(value);
    }

    if vm.is_string_value(value) {
        return Ok(BxValue::new_ptr(vm.string_new(vm.to_string(value))));
    }

    if vm.is_bytes(value) {
        return Ok(BxValue::new_ptr(vm.bytes_new(vm.to_bytes(value)?)));
    }

    if let Some(type_name) = vm.type_name_from_value(value) {
        if type_name.eq_ignore_ascii_case("datetime") {
            let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
            return Ok(BxValue::new_ptr(vm.datetime_new(dt)));
        }
    }

    let Some(id) = value.as_gc_id() else {
        return Ok(value);
    };

    if let Some(existing) = seen.get(&id) {
        return Ok(*existing);
    }

    if vm.is_array_value(value) {
        let new_id = vm.array_new();
        let duplicated = BxValue::new_ptr(new_id);
        seen.insert(id, duplicated);
        for idx in 0..vm.array_len(id) {
            let item = vm.array_get(id, idx);
            let copied = if deep {
                duplicate_value(vm, item, true, seen)?
            } else {
                item
            };
            vm.array_push(new_id, copied);
        }
        return Ok(duplicated);
    }

    if vm.is_struct_value(value) {
        let new_id = vm.struct_new();
        let duplicated = BxValue::new_ptr(new_id);
        seen.insert(id, duplicated);
        for key in vm.struct_key_array(id) {
            let item = vm.struct_get(id, &key);
            let copied = if deep {
                duplicate_value(vm, item, true, seen)?
            } else {
                item
            };
            vm.struct_set(new_id, &key, copied);
        }
        return Ok(duplicated);
    }

    Ok(value)
}

// --- System BIFs ---

fn create_uuid(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = Uuid::new_v4().to_string().to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(id)))
}

fn create_guid(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = Uuid::new_v4().to_string().to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(id)))
}

fn get_system_setting(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("getSystemSetting() expects at least 1 argument".to_string());
    }
    let key = vm.to_string(args[0]);

    match std::env::var(&key) {
        Ok(val) => Ok(BxValue::new_ptr(vm.string_new(val))),
        Err(_) => {
            if args.len() > 1 {
                Ok(args[1])
            } else {
                Ok(BxValue::new_null())
            }
        }
    }
}

// --- Array BIFs ---

fn array_append(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("arrayAppend() expects exactly 2 arguments".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_push(id, args[1]);
        Ok(BxValue::new_bool(true))
    } else {
        Err("arrayAppend() expects an array as the first argument".to_string())
    }
}

fn array_append_member(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    array_append(vm, args)?;
    Ok(args[0])
}

fn array_prepend(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("arrayPrepend() expects exactly 2 arguments".to_string());
    }
    let id = args[0]
        .as_gc_id()
        .ok_or_else(|| "arrayPrepend() expects an array as the first argument".to_string())?;
    vm.array_insert_at(id, 0, args[1])?;
    Ok(args[0])
}

fn array_map(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("arrayMap() expects an array and callback".to_string());
    }
    let array_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "arrayMap() expects an array as the first argument".to_string())?;
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "arrayMap() requires an active execution chunk".to_string())?;
    let result_id = vm.array_new();
    for index in 0..vm.array_len(array_id) {
        let value = vm.array_get(array_id, index);
        let mapped = vm.call_function_by_value(
            &args[1],
            vec![value, BxValue::new_number((index + 1) as f64)],
            chunk.clone(),
        )?;
        vm.array_push(result_id, mapped);
    }
    Ok(BxValue::new_ptr(result_id))
}

fn array_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.array_new();
    let key = (vm as *mut dyn BxVM as *mut () as usize, id);
    let dimensions = args
        .first()
        .filter(|value| value.is_number() || value.is_int())
        .map(|value| value.as_number() as usize)
        .unwrap_or(1)
        .max(1);
    ARRAY_DIMENSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Array dimension metadata lock poisoned".to_string())?
        .insert(key, dimensions);
    let arrays = UNSYNCHRONIZED_ARRAYS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut arrays = arrays
        .lock()
        .map_err(|_| "Array metadata lock poisoned".to_string())?;
    if args.len() > 1 && !args[1].as_bool() {
        arrays.insert(key);
    } else {
        arrays.remove(&key);
    }
    Ok(BxValue::new_ptr(id))
}

fn array_is_synchronized_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || !vm.is_array_value(args[0]) {
        return Ok(BxValue::new_bool(true));
    }
    let id = args[0]
        .as_gc_id()
        .ok_or_else(|| "arrayIsSynchronized() expects an array".to_string())?;
    let key = (vm as *mut dyn BxVM as *mut () as usize, id);
    let arrays = UNSYNCHRONIZED_ARRAYS.get_or_init(|| Mutex::new(HashSet::new()));
    let arrays = arrays
        .lock()
        .map_err(|_| "Array metadata lock poisoned".to_string())?;
    Ok(BxValue::new_bool(!arrays.contains(&key)))
}

fn array_pop_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("arrayPop() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        if vm.array_len(id) == 0 && args.len() > 1 {
            return Ok(args[1]);
        }
        vm.array_pop(id)
    } else {
        Err("arrayPop() expects an array".to_string())
    }
}

fn array_delete_at_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("arrayDeleteAt() expects 2 arguments: (array, index)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Array index must be 1-based".to_string());
        }
        vm.array_delete_at(id, idx - 1)
    } else {
        Err("arrayDeleteAt() expects an array as the first argument".to_string())
    }
}

fn array_insert_at_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("arrayInsertAt() expects 3 arguments: (array, index, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Array index must be 1-based".to_string());
        }
        vm.array_insert_at(id, idx - 1, args[2])?;
        Ok(args[0])
    } else {
        Err("arrayInsertAt() expects an array as the first argument".to_string())
    }
}

fn array_clear_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("arrayClear() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_clear(id)?;
        Ok(BxValue::new_bool(true))
    } else {
        Err("arrayClear() expects an array".to_string())
    }
}

fn array_swap_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("arraySwap() expects 3 arguments: (array, fromIndex, toIndex)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let from = args[1].as_number() as usize;
        let to = args[2].as_number() as usize;
        if from == 0 || to == 0 {
            return Err("Array index must be 1-based".to_string());
        }
        let from_idx = from - 1;
        let to_idx = to - 1;
        let len = vm.array_len(id);
        if from_idx >= len || to_idx >= len {
            return Err("arraySwap() index out of bounds".to_string());
        }
        let left = vm.array_get(id, from_idx);
        let right = vm.array_get(id, to_idx);
        vm.array_set(id, from_idx, right)?;
        vm.array_set(id, to_idx, left)?;
        Ok(args[0])
    } else {
        Err("arraySwap() expects an array as the first argument".to_string())
    }
}

fn array_resize_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("arrayResize() expects 2 arguments: (array, size)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let size = args[1].as_number() as isize;
        if size < 0 {
            return Err("arrayResize() size must be non-negative".to_string());
        }
        let size = size as usize;
        let current = vm.array_len(id);
        if size > current {
            for _ in current..size {
                vm.array_push(id, BxValue::new_null());
            }
        }
        Ok(args[0])
    } else {
        Err("arrayResize() expects an array as the first argument".to_string())
    }
}

fn array_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("arraySet() expects 3 arguments: (array, index, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let from = args[1].as_number() as usize;
        let (to, value) = if args.len() >= 4 {
            (args[2].as_number() as usize, args[3])
        } else {
            (from, args[2])
        };
        if from == 0 || to == 0 || to < from {
            return Err("Array index must be 1-based".to_string());
        }
        for idx in from..=to {
            vm.array_set(id, idx - 1, value)?;
        }
        Ok(args[0])
    } else {
        Err("arraySet() expects an array as the first argument".to_string())
    }
}

fn bytes_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("bytesNew() expects 1 argument: (array)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let len = vm.array_len(id);
        let mut out = Vec::with_capacity(len);
        for idx in 0..len {
            let value = vm.array_get(id, idx);
            if !value.is_number() {
                return Err("bytesNew() expects an array of byte values".to_string());
            }
            let byte = value.as_number();
            if !(0.0..=255.0).contains(&byte) || byte.fract() != 0.0 {
                return Err(
                    "bytesNew() byte values must be integers in the range 0..255".to_string(),
                );
            }
            out.push(byte as u8);
        }
        Ok(BxValue::new_ptr(vm.bytes_new(out)))
    } else {
        Err("bytesNew() expects an array".to_string())
    }
}

fn bytes_len_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("bytesLen() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_number(vm.bytes_len(id) as f64))
    } else {
        Err("bytesLen() expects bytes".to_string())
    }
}

fn bytes_get_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("bytesGet() expects 2 arguments: (bytes, index)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Byte index must be 1-based".to_string());
        }
        Ok(BxValue::new_number(vm.bytes_get(id, idx - 1)? as f64))
    } else {
        Err("bytesGet() expects bytes as the first argument".to_string())
    }
}

fn bytes_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 3 {
        return Err("bytesSet() expects 3 arguments: (bytes, index, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Byte index must be 1-based".to_string());
        }
        if !args[2].is_number() {
            return Err("bytesSet() expects a numeric byte value".to_string());
        }
        let value = args[2].as_number();
        if !(0.0..=255.0).contains(&value) || value.fract() != 0.0 {
            return Err("bytesSet() byte values must be integers in the range 0..255".to_string());
        }
        vm.bytes_set(id, idx - 1, value as u8)?;
        Ok(args[0])
    } else {
        Err("bytesSet() expects bytes as the first argument".to_string())
    }
}

fn is_bytes_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(vm.is_bytes(args[0])))
}

// --- Struct BIFs ---

fn struct_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let options = args
        .iter()
        .map(|value| vm.to_string(*value))
        .collect::<Vec<_>>();
    Ok(BxValue::new_ptr(vm.struct_new_with_options(&options)))
}

fn struct_insert_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("structInsert() expects 3 arguments: (struct, key, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        let overwrite = args
            .get(3)
            .is_some_and(|value| value.is_bool() && value.as_bool());
        if vm.struct_key_exists(id, &key) && !overwrite {
            return Err(format!("Key '{}' already exists in struct", key));
        }
        vm.struct_set(id, &key, args[2]);
        Ok(args[0])
    } else {
        Err("structInsert() expects a struct as the first argument".to_string())
    }
}

fn struct_update_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("structUpdate() expects 3 arguments: (struct, key, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        vm.struct_set(id, &key, args[2]);
        Ok(args[0])
    } else {
        Err("structUpdate() expects a struct as the first argument".to_string())
    }
}

fn struct_get_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() == 1 {
        let path = vm.to_string(args[0]);
        return Ok(vm
            .resolve_variable_path(&path)
            .unwrap_or(BxValue::new_null()));
    }
    if args.len() < 2 {
        return Err("structGet() expects 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(vm.struct_get(id, &key))
    } else {
        Err("structGet() expects a struct as the first argument".to_string())
    }
}

fn struct_delete_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structDelete() expects 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(BxValue::new_bool(vm.struct_delete(id, &key)))
    } else {
        Err("structDelete() expects a struct as the first argument".to_string())
    }
}

fn struct_key_exists_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structKeyExists() expects 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(BxValue::new_bool(vm.struct_key_exists(id, &key)))
    } else {
        Err("structKeyExists() expects a struct as the first argument".to_string())
    }
}

fn struct_key_array_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structKeyArray() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let keys = vm.struct_key_array(id);
        let arr_id = vm.array_new();
        for key in keys {
            let s_id = vm.string_new(key);
            vm.array_push(arr_id, BxValue::new_ptr(s_id));
        }
        Ok(BxValue::new_ptr(arr_id))
    } else {
        Err("structKeyArray() expects a struct".to_string())
    }
}

fn struct_clear_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structClear() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.struct_clear(id);
        Ok(BxValue::new_bool(true))
    } else {
        Err("structClear() expects a struct".to_string())
    }
}

fn struct_find_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structFind() expects at least 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        let found = vm.struct_get(id, &key);
        if found.is_null() {
            if args.len() >= 3 {
                Ok(args[2])
            } else {
                Err(format!("Key '{}' not found in struct", key))
            }
        } else {
            Ok(found)
        }
    } else {
        Err("structFind() expects a struct as the first argument".to_string())
    }
}

fn struct_is_empty_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structIsEmpty() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_bool(vm.struct_len(id) == 0))
    } else {
        Err("structIsEmpty() expects a struct".to_string())
    }
}

fn struct_is_case_sensitive_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structIsCaseSensitive() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_bool(vm.struct_is_case_sensitive(id)))
    } else {
        Err("structIsCaseSensitive() expects a struct".to_string())
    }
}

fn struct_is_ordered_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structIsOrdered() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_bool(vm.struct_is_ordered(id)))
    } else {
        Err("structIsOrdered() expects a struct".to_string())
    }
}

fn struct_equals_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structEquals() expects 2 arguments: (struct1, struct2)".to_string());
    }
    let id1 = args[0]
        .as_gc_id()
        .ok_or("structEquals() expects a struct as the first argument")?;
    let id2 = args[1]
        .as_gc_id()
        .ok_or("structEquals() expects a struct as the second argument")?;
    let len1 = vm.struct_len(id1);
    let len2 = vm.struct_len(id2);
    if len1 != len2 {
        return Ok(BxValue::new_bool(false));
    }
    let keys1 = vm.struct_key_array(id1);
    for key in &keys1 {
        if !vm.struct_key_exists(id2, key) {
            return Ok(BxValue::new_bool(false));
        }
        let v1 = vm.struct_get(id1, key);
        let v2 = vm.struct_get(id2, key);
        if !values_equal(vm, v1, v2) {
            return Ok(BxValue::new_bool(false));
        }
    }
    Ok(BxValue::new_bool(true))
}

fn values_equal(vm: &mut dyn BxVM, a: BxValue, b: BxValue) -> bool {
    if a.is_ptr() && b.is_ptr() {
        let id_a = a.as_gc_id().unwrap();
        let id_b = b.as_gc_id().unwrap();
        if vm.is_struct_value(a) && vm.is_struct_value(b) {
            let len_a = vm.struct_len(id_a);
            let len_b = vm.struct_len(id_b);
            if len_a != len_b {
                return false;
            }
            let keys = vm.struct_key_array(id_a);
            for key in &keys {
                if !vm.struct_key_exists(id_b, key) {
                    return false;
                }
                let va = vm.struct_get(id_a, key);
                let vb = vm.struct_get(id_b, key);
                if !values_equal(vm, va, vb) {
                    return false;
                }
            }
            return true;
        }
        if vm.is_array_value(a) && vm.is_array_value(b) {
            let len_a = vm.array_len(id_a);
            let len_b = vm.array_len(id_b);
            if len_a != len_b {
                return false;
            }
            for i in 0..len_a {
                let va = vm.array_get(id_a, i);
                let vb = vm.array_get(id_b, i);
                if !values_equal(vm, va, vb) {
                    return false;
                }
            }
            return true;
        }
    }
    a == b
}

fn struct_get_metadata_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structGetMetadata() expects 1 argument".to_string());
    }
    if args[0].as_gc_id().is_none() {
        return Err("structGetMetadata() expects a struct".to_string());
    }
    let meta_id = vm.struct_new();
    vm.struct_set(meta_id, "casesensitive", BxValue::new_bool(false));
    vm.struct_set(meta_id, "ordered", BxValue::new_bool(true));
    vm.struct_set(meta_id, "unmodifiable", BxValue::new_bool(false));
    vm.struct_set(meta_id, "soft", BxValue::new_bool(false));
    let type_str = vm.string_new("linked".to_string());
    vm.struct_set(meta_id, "type", BxValue::new_ptr(type_str));
    Ok(BxValue::new_ptr(meta_id))
}

fn percent_encode(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

fn struct_to_query_string_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structToQueryString() expects at least 1 argument".to_string());
    }
    let id = args[0]
        .as_gc_id()
        .ok_or("structToQueryString() expects a struct as the first argument")?;
    let delimiter = if args.len() >= 2 {
        vm.to_string(args[1])
    } else {
        "&".to_string()
    };
    let keys = vm.struct_key_array(id);
    let mut parts = Vec::new();
    for key in &keys {
        let val = vm.struct_get(id, key);
        let val_str = vm.to_string(val);
        parts.push(format!(
            "{}={}",
            percent_encode(key),
            percent_encode(&val_str)
        ));
    }
    let qs = parts.join(&delimiter);
    let qs_id = vm.string_new(qs);
    Ok(BxValue::new_ptr(qs_id))
}

fn struct_to_sorted_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structToSorted() expects at least 1 argument".to_string());
    }
    let id = args[0]
        .as_gc_id()
        .ok_or("structToSorted() expects a struct as the first argument")?;
    let sort_order_desc = if args.len() >= 3 {
        let order = vm.to_string(args[2]);
        order.eq_ignore_ascii_case("desc")
    } else {
        false
    };
    let keys = vm.struct_key_array(id);
    let mut sorted_keys: Vec<(String, usize)> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (k.to_lowercase(), i))
        .collect();
    sorted_keys.sort_by(|a, b| a.0.cmp(&b.0));
    if sort_order_desc {
        sorted_keys.reverse();
    }
    let new_id = vm.struct_new();
    for (_, orig_idx) in &sorted_keys {
        let key = &keys[*orig_idx];
        let val = vm.struct_get(id, key);
        vm.struct_set(new_id, key, val);
    }
    Ok(BxValue::new_ptr(new_id))
}

fn struct_key_translate_path(vm: &mut dyn BxVM, struct_id: usize, parts: &[&str], value: BxValue) {
    if parts.len() == 1 {
        vm.struct_set(struct_id, parts[0], value);
        return;
    }

    let parent_id = match vm.struct_get(struct_id, parts[0]).as_gc_id() {
        Some(id) if vm.is_struct_value(vm.struct_get(struct_id, parts[0])) => id,
        _ => {
            let id = vm.struct_new();
            vm.struct_set(struct_id, parts[0], BxValue::new_ptr(id));
            id
        }
    };
    struct_key_translate_path(vm, parent_id, &parts[1..], value);
}

fn struct_key_translate_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structKeyTranslate() expects at least 1 argument".to_string());
    }
    let id = args[0]
        .as_gc_id()
        .ok_or("structKeyTranslate() expects a struct as the first argument")?;
    let deep = args.len() >= 2 && args[1] == BxValue::new_bool(true);
    let retain_keys = args.len() >= 3 && args[2] == BxValue::new_bool(true);
    let keys = vm.struct_key_array(id);
    let dot_keys: Vec<String> = keys.iter().filter(|k| k.contains('.')).cloned().collect();
    for dot_key in &dot_keys {
        let val = vm.struct_get(id, dot_key);
        let parts: Vec<&str> = if deep {
            dot_key.split('.').collect()
        } else {
            dot_key.splitn(2, '.').collect()
        };
        if parts.len() >= 2 {
            struct_key_translate_path(vm, id, &parts, val);
            if !retain_keys {
                vm.struct_delete(id, dot_key);
            }
        }
    }
    Ok(args[0])
}

fn struct_find_key_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structFindKey() expects at least 2 arguments: (struct, key)".to_string());
    }
    let id = args[0]
        .as_gc_id()
        .ok_or("structFindKey() expects a struct as the first argument")?;
    let search_key = vm.to_string(args[1]);
    let scope_all = args.len() >= 3 && vm.to_string(args[2]).eq_ignore_ascii_case("all");
    let results_id = vm.array_new();
    if search_key.contains('.') {
        let parts: Vec<&str> = search_key.splitn(2, '.').collect();
        let first_key = parts[0];
        let rest_key = parts[1];
        let val = vm.struct_get(id, first_key);
        if !val.is_null() {
            if let Some(nested_id) = val.as_gc_id() {
                if vm.is_struct_value(val) {
                    let rest_str = BxValue::new_ptr(vm.string_new(rest_key.to_string()));
                    let scope_str = BxValue::new_ptr(
                        vm.string_new(if scope_all { "all" } else { "one" }.to_string()),
                    );
                    let nested_args = vec![BxValue::new_ptr(nested_id), rest_str, scope_str];
                    let nested_result = struct_find_key_bif(vm, &nested_args)?;
                    if let Some(nested_arr_id) = nested_result.as_gc_id() {
                        let nested_len = vm.array_len(nested_arr_id);
                        for i in 0..nested_len {
                            let item = vm.array_get(nested_arr_id, i);
                            if let Some(item_id) = item.as_gc_id() {
                                if vm.is_struct_value(item) {
                                    let path = vm.struct_get(item_id, "path");
                                    let path_str = vm.to_string(path);
                                    let new_path =
                                        vm.string_new(format!("{}.{}", first_key, path_str));
                                    vm.struct_set(item_id, "path", BxValue::new_ptr(new_path));
                                }
                            }
                            vm.array_push(results_id, item);
                            if !scope_all {
                                break;
                            }
                        }
                    }
                }
            }
        }
    } else {
        let keys = vm.struct_key_array(id);
        for key in &keys {
            if key.eq_ignore_ascii_case(&search_key) {
                let val = vm.struct_get(id, key);
                let entry_id = vm.struct_new();
                vm.struct_set(entry_id, "owner", BxValue::new_ptr(id));
                let path_str = vm.string_new(key.clone());
                vm.struct_set(entry_id, "path", BxValue::new_ptr(path_str));
                vm.struct_set(entry_id, "value", val);
                vm.array_push(results_id, BxValue::new_ptr(entry_id));
                if !scope_all {
                    break;
                }
            }
            let val = vm.struct_get(id, key);
            if !scope_all && vm.array_len(results_id) > 0 {
                break;
            }
            if let Some(nested_id) = val.as_gc_id() {
                if vm.is_struct_value(val) {
                    let rest_str = BxValue::new_ptr(vm.string_new(search_key.clone()));
                    let scope_str = BxValue::new_ptr(
                        vm.string_new(if scope_all { "all" } else { "one" }.to_string()),
                    );
                    let nested_args = vec![BxValue::new_ptr(nested_id), rest_str, scope_str];
                    let nested_result = struct_find_key_bif(vm, &nested_args)?;
                    if let Some(nested_arr_id) = nested_result.as_gc_id() {
                        let nested_len = vm.array_len(nested_arr_id);
                        for i in 0..nested_len {
                            let item = vm.array_get(nested_arr_id, i);
                            if let Some(item_id) = item.as_gc_id() {
                                if vm.is_struct_value(item) {
                                    let path = vm.struct_get(item_id, "path");
                                    let path_str = vm.to_string(path);
                                    let new_path = vm.string_new(format!("{}.{}", key, path_str));
                                    vm.struct_set(item_id, "path", BxValue::new_ptr(new_path));
                                    vm.struct_set(item_id, "owner", BxValue::new_ptr(nested_id));
                                }
                            }
                            vm.array_push(results_id, item);
                            if !scope_all {
                                break;
                            }
                        }
                    }
                }
            }
            if !scope_all && vm.array_len(results_id) > 0 {
                break;
            }
        }
    }
    Ok(BxValue::new_ptr(results_id))
}

// --- Date/Time BIFs ---

pub(super) fn parse_timezone_offset(tz: Option<&str>) -> Option<FixedOffset> {
    let tz = tz?.trim();
    let normalized = tz.to_ascii_lowercase();
    let named_offset = match normalized.as_str() {
        "america/new_york" => -4 * 3600,
        "america/chicago" => -5 * 3600,
        "america/los_angeles" => -8 * 3600,
        "europe/zurich" | "europe/berlin" => 1 * 3600,
        "europe/london" => 0,
        "asia/tokyo" => 9 * 3600,
        "australia/sydney" => 10 * 3600,
        "pst" => -8 * 3600,
        "mst" => -7 * 3600,
        "cst" => -6 * 3600,
        "est" => -5 * 3600,
        "us/hawaii" | "pacific/honolulu" => -10 * 3600,
        _ => 0,
    };
    if matches!(
        normalized.as_str(),
        "america/new_york"
            | "america/chicago"
            | "america/los_angeles"
            | "europe/zurich"
            | "europe/berlin"
            | "europe/london"
            | "asia/tokyo"
            | "australia/sydney"
            | "pst"
            | "mst"
            | "cst"
            | "est"
            | "us/hawaii"
            | "pacific/honolulu"
    ) {
        return FixedOffset::east_opt(named_offset);
    }
    if tz.is_empty()
        || tz.eq_ignore_ascii_case("utc")
        || tz.eq_ignore_ascii_case("gmt")
        || tz == "z"
    {
        return FixedOffset::east_opt(0);
    }

    let raw = tz
        .strip_prefix("UTC")
        .or_else(|| tz.strip_prefix("utc"))
        .or_else(|| tz.strip_prefix("GMT"))
        .or_else(|| tz.strip_prefix("gmt"))
        .unwrap_or(tz);
    let raw = raw.trim();
    if raw.is_empty() {
        return FixedOffset::east_opt(0);
    }

    let sign = match raw.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits = &raw[1..];
    let digits = digits.replace(':', "");
    if digits.len() != 2 && digits.len() != 4 {
        return None;
    }
    let hours: i32 = digits[0..2].parse().ok()?;
    let mins: i32 = if digits.len() == 4 {
        digits[2..4].parse().ok()?
    } else {
        0
    };
    let total = sign * (hours * 3600 + mins * 60);
    FixedOffset::east_opt(total)
}

fn translate_datetime_format(fmt: &str) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            while let Some(next) = chars.next() {
                if next == '\'' {
                    break;
                }
                out.push(next);
            }
            continue;
        }

        if ch.is_ascii_alphabetic() {
            let mut count = 1usize;
            while chars.peek().copied() == Some(ch) {
                count += 1;
                chars.next();
            }
            let token = match (ch, count) {
                ('Y', 4) => "%Y",
                ('Y', 2) => "%y",
                ('y', 4) => "%Y",
                ('y', 2) => "%y",
                ('M', 4) => "%B",
                ('M', 3) => "%b",
                ('M', 2) => "%m",
                ('M', 1) => "%-m",
                ('d', 2) => "%d",
                ('d', 1) => "%-d",
                ('E', 4) => "%A",
                ('E', 3) => "%a",
                ('H', 2) => "%H",
                ('H', 1) => "%-H",
                ('h', 2) => "%I",
                ('h', 1) => "%-I",
                ('m', 2) => "%M",
                ('m', 1) => "%-M",
                ('s', 2) => "%S",
                ('s', 1) => "%-S",
                ('S', n) => {
                    if out.ends_with('.') {
                        out.pop();
                    }
                    out.push_str(&format!("%.{}f", n));
                    continue;
                }
                ('X', 1) => "%:z",
                ('X', 2) => "%:z",
                ('X', 3) => "%:z",
                ('Z', _) => "%z",
                ('v', _) => "%:z",
                ('a', _) => "%p",
                _ => {
                    for _ in 0..count {
                        out.push(ch);
                    }
                    continue;
                }
            };
            out.push_str(token);
        } else {
            out.push(ch);
        }
    }
    out
}

pub(super) fn format_datetime(
    dt: DateTime<Utc>,
    format: Option<&str>,
    default_format: &str,
    tz: Option<&str>,
) -> Result<String, String> {
    let format = format.unwrap_or(default_format).trim();
    if format.eq_ignore_ascii_case("n") {
        return Ok(dt.nanosecond().to_string());
    }
    if format.eq_ignore_ascii_case("epoch") {
        return Ok(dt.timestamp().to_string());
    }
    if format.eq_ignore_ascii_case("epochms") {
        return Ok(dt.timestamp_millis().to_string());
    }
    if format.eq_ignore_ascii_case("z") {
        return Ok(match tz.unwrap_or("UTC") {
            "America/Los_Angeles" => "PDT".to_string(),
            "US/Hawaii" | "Pacific/Honolulu" => "HST".to_string(),
            "America/New_York" => "EDT".to_string(),
            _ => "UTC".to_string(),
        });
    }
    if format.eq_ignore_ascii_case("yyyy-MM-dd'T'HH:mm:ss.SSSX") {
        let formatted = if let Some(offset) = parse_timezone_offset(tz) {
            dt.with_timezone(&offset)
                .format("%Y-%m-%dT%H:%M:%S%.3f%:z")
                .to_string()
        } else {
            dt.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string()
        };
        return Ok(if formatted.ends_with("+00:00") {
            formatted.trim_end_matches("+00:00").to_string() + "Z"
        } else if formatted.ends_with("+0000") {
            formatted.trim_end_matches("+0000").to_string() + "Z"
        } else {
            formatted
        });
    }
    let (format, alias) = match format.to_ascii_lowercase().as_str() {
        "short" if default_format.contains("HH") => (Some("MM/dd/yy, hh:mm a"), None),
        "short" => (Some("dd-MMM-yy"), None),
        "long" => (Some("MMMM d, yyyy"), None),
        "iso" | "iso8601" => (Some("yyyy-MM-dd"), None),
        "full" => (Some("EEEE, MMMM d, yyyy"), None),
        _ => (None, Some(format)),
    };

    let chrono_format = if let Some(alias_fmt) = format {
        translate_datetime_format(alias_fmt)
    } else {
        translate_datetime_format(alias.unwrap())
    };

    let formatted = if let Some(offset) = parse_timezone_offset(tz) {
        dt.with_timezone(&offset).format(&chrono_format).to_string()
    } else {
        dt.format(&chrono_format).to_string()
    };

    if format.is_none() && alias.unwrap_or("").contains('X') {
        if formatted.ends_with("+00:00") {
            Ok(formatted.trim_end_matches("+00:00").to_string() + "Z")
        } else if formatted.ends_with("+0000") {
            Ok(formatted.trim_end_matches("+0000").to_string() + "Z")
        } else {
            Ok(formatted)
        }
    } else {
        Ok(formatted)
    }
}

fn parse_datetime_with_format(
    input: &str,
    format: &str,
    tz: Option<&str>,
) -> Result<DateTime<Utc>, String> {
    let mut chrono_format = translate_datetime_format(format);
    for digits in 1..=9 {
        chrono_format = chrono_format.replace(&format!("%.{}f", digits), "%.f");
    }
    let has_offset = chrono_format.contains("%:z") || chrono_format.contains("%z");
    if has_offset {
        let normalized = if input.ends_with('Z') {
            format!("{}+00:00", &input[..input.len().saturating_sub(1)])
        } else if input.len() >= 5 {
            let suffix = &input[input.len() - 5..];
            if (suffix.starts_with('+') || suffix.starts_with('-'))
                && suffix[1..].chars().all(|c| c.is_ascii_digit())
            {
                format!(
                    "{}{}:{}",
                    &input[..input.len() - 5],
                    &suffix[..3],
                    &suffix[3..5]
                )
            } else {
                input.to_string()
            }
        } else {
            input.to_string()
        };
        let parsed = DateTime::parse_from_str(&normalized, &chrono_format)
            .or_else(|_| DateTime::parse_from_str(input, &chrono_format))
            .map_err(|e| e.to_string())?;
        Ok(parsed.with_timezone(&Utc))
    } else if chrono_format.contains("%Y")
        || chrono_format.contains("%m")
        || chrono_format.contains("%d")
    {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(input, &chrono_format) {
            let offset =
                parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            Ok(offset
                .from_local_datetime(&parsed)
                .single()
                .unwrap()
                .with_timezone(&Utc))
        } else if let Ok(parsed) = NaiveDate::parse_from_str(input, &chrono_format) {
            let naive = parsed.and_hms_opt(0, 0, 0).unwrap();
            let offset =
                parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            Ok(offset
                .from_local_datetime(&naive)
                .single()
                .unwrap()
                .with_timezone(&Utc))
        } else {
            Err(format!("Unable to parse date '{}'", input))
        }
    } else {
        Err(format!("Unsupported date format '{}'", format))
    }
}

fn parse_named_month_datetime(input: &str, tz: Option<&str>) -> Option<DateTime<Utc>> {
    let cleaned = input.replace(',', "");
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    let months = [
        "jan",
        "january",
        "feb",
        "february",
        "mar",
        "march",
        "apr",
        "april",
        "may",
        "jun",
        "june",
        "jul",
        "july",
        "aug",
        "august",
        "sep",
        "september",
        "oct",
        "october",
        "nov",
        "november",
        "dec",
        "december",
    ];
    let month_index = parts
        .iter()
        .position(|part| months.iter().any(|month| part.eq_ignore_ascii_case(month)))?;
    let month = months
        .iter()
        .position(|month| month.eq_ignore_ascii_case(parts[month_index]))? as u32
        / 2
        + 1;
    let day = parts[..month_index]
        .iter()
        .rev()
        .find_map(|part| {
            part.parse::<u32>()
                .ok()
                .filter(|day| (1..=31).contains(day))
        })
        .or_else(|| parts.get(month_index + 1)?.parse::<u32>().ok())?;
    let year = parts
        .iter()
        .find_map(|part| part.parse::<i32>().ok().filter(|year| *year >= 1000))?;
    let time = parts
        .iter()
        .find(|part| part.matches(':').count() >= 1)
        .copied()?;
    let time = NaiveTime::parse_from_str(time, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(time, "%H:%M"))
        .ok()?;
    let timezone = parts.iter().find(|part| {
        matches!(
            part.to_ascii_uppercase().as_str(),
            "UTC" | "GMT" | "CET" | "CEST" | "PST" | "PDT"
        )
    });
    if timezone.is_none()
        && parts
            .iter()
            .any(|part| part.to_ascii_uppercase().starts_with("GMT"))
    {
        return None;
    }
    let offset = match timezone.map(|value| value.to_ascii_uppercase()).as_deref() {
        Some("CET") => FixedOffset::east_opt(3600),
        Some("CEST") => FixedOffset::east_opt(7200),
        Some("PST") => FixedOffset::west_opt(8 * 3600),
        Some("PDT") => FixedOffset::west_opt(7 * 3600),
        Some("UTC") | Some("GMT") => FixedOffset::east_opt(0),
        _ => parse_timezone_offset(tz),
    }?;
    let date = NaiveDate::from_ymd_opt(year, month, day)?.and_time(time);
    offset
        .from_local_datetime(&date)
        .single()
        .map(|value| value.with_timezone(&Utc))
}

pub(super) fn parse_datetime_input(
    input: &str,
    format: Option<&str>,
    tz: Option<&str>,
) -> Result<DateTime<Utc>, String> {
    if let Some(format) = format {
        if let Some(parsed) = parse_named_month_datetime(input, tz) {
            return Ok(parsed);
        }
        if format.contains('S') && format.contains('X') {
            return parse_datetime_input(input, None, tz);
        }
        return parse_datetime_with_format(input, format, tz);
    }

    if let Some(parsed) = parse_named_month_datetime(input, tz) {
        return Ok(parsed);
    }

    if let Some(paren) = input.find(" (") {
        let value = &input[..paren];
        let parts: Vec<&str> = value.split_whitespace().collect();
        if parts.len() == 6 && parts[4].contains(':') && parts[5].starts_with("GMT") {
            let date = NaiveDate::parse_from_str(
                &format!("{} {} {}", parts[1], parts[2], parts[3]),
                "%b %d %Y",
            );
            let time = NaiveTime::parse_from_str(parts[4], "%H:%M:%S");
            if let (Ok(date), Ok(time), Some(offset)) =
                (date, time, parse_timezone_offset(Some(parts[5])))
            {
                return Ok(offset
                    .from_local_datetime(&date.and_time(time))
                    .single()
                    .ok_or_else(|| format!("Unable to parse date '{}'", input))?
                    .with_timezone(&Utc));
            }
        }
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(input) {
        return Ok(parsed.with_timezone(&Utc));
    }

    let normalized = if input.ends_with('Z') {
        format!("{}+00:00", &input[..input.len().saturating_sub(1)])
    } else if input.len() >= 5 {
        let suffix = &input[input.len() - 5..];
        if (suffix.starts_with('+') || suffix.starts_with('-'))
            && suffix[1..].chars().all(|c| c.is_ascii_digit())
        {
            format!(
                "{}{}:{}",
                &input[..input.len() - 5],
                &suffix[..3],
                &suffix[3..5]
            )
        } else {
            input.to_string()
        }
    } else {
        input.to_string()
    };
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&normalized) {
        return Ok(parsed.with_timezone(&Utc));
    }

    let naive_patterns = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
        "%Y.%m.%d",
        "%d.%m.%Y",
        "%Y/%m/%d",
        "%m/%d/%Y",
        "%b %d, %Y %H:%M:%S",
        "%B %d, %Y %H:%M:%S",
        "%b %d %Y %H:%M",
        "%B %d %Y %H:%M",
        "%B, %d %Y",
        "%b, %d %Y",
        "%B %d %Y",
        "%b %d %Y",
        "%m/%Y",
    ];
    let meridian_patterns = [
        "%Y-%m-%d %I:%M:%S %p",
        "%b-%d-%Y %I:%M%p",
        "%m/%d/%Y %I:%M %p",
    ];
    let lower_input = input.to_ascii_lowercase();
    if lower_input.ends_with(" am")
        || lower_input.ends_with(" pm")
        || lower_input.ends_with("am")
        || lower_input.ends_with("pm")
    {
        let is_pm = lower_input.ends_with("pm") || lower_input.ends_with(" pm");
        let core = input[..input.len() - if input.ends_with(' ') { 3 } else { 2 }].trim();
        let parts: Vec<&str> = core.split_whitespace().collect();
        if parts.len() == 2 {
            let date = if parts[0].contains('/') {
                let values: Vec<&str> = parts[0].split('/').collect();
                if values.len() == 3 {
                    values[0].parse::<u32>().ok().and_then(|month| {
                        values[1].parse::<u32>().ok().and_then(|day| {
                            values[2]
                                .parse::<i32>()
                                .ok()
                                .and_then(|year| NaiveDate::from_ymd_opt(year, month, day))
                        })
                    })
                } else {
                    None
                }
            } else {
                NaiveDate::parse_from_str(parts[0], "%Y-%m-%d")
                    .or_else(|_| NaiveDate::parse_from_str(parts[0], "%b-%d-%Y"))
                    .ok()
            };
            let time_parts: Vec<&str> = parts[1].split(':').collect();
            if let (Some(date), Some(hour), Some(minute)) = (
                date,
                time_parts.first().and_then(|v| v.parse::<u32>().ok()),
                time_parts.get(1).and_then(|v| v.parse::<u32>().ok()),
            ) {
                let hour = if is_pm {
                    if hour == 12 { 12 } else { hour + 12 }
                } else if hour == 12 {
                    0
                } else {
                    hour
                };
                if let Some(naive) = date.and_hms_opt(hour, minute, 0) {
                    let offset = parse_timezone_offset(tz)
                        .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
                    return Ok(offset
                        .from_local_datetime(&naive)
                        .single()
                        .unwrap()
                        .with_timezone(&Utc));
                }
            }
        }
    }
    for pattern in meridian_patterns {
        let parsed = NaiveDateTime::parse_from_str(input, pattern)
            .or_else(|_| NaiveDateTime::parse_from_str(&input.to_ascii_uppercase(), pattern));
        if let Ok(parsed) = parsed {
            let offset =
                parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            return Ok(offset
                .from_local_datetime(&parsed)
                .single()
                .unwrap()
                .with_timezone(&Utc));
        }
    }
    if let Some((month, year)) = input
        .split_once('/')
        .and_then(|(month, year)| Some((month.parse::<u32>().ok()?, year.parse::<i32>().ok()?)))
    {
        let date = NaiveDate::from_ymd_opt(year, month, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let offset = parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
        return Ok(offset
            .from_local_datetime(&date)
            .single()
            .unwrap()
            .with_timezone(&Utc));
    }
    for pattern in ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"] {
        if let Ok(parsed) = NaiveTime::parse_from_str(input, pattern) {
            let date = NaiveDate::from_ymd_opt(1970, 1, 1)
                .unwrap()
                .and_time(parsed);
            let offset =
                parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            return Ok(offset
                .from_local_datetime(&date)
                .single()
                .unwrap()
                .with_timezone(&Utc));
        }
    }
    for pattern in naive_patterns {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(input, pattern) {
            let offset =
                parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            return Ok(offset
                .from_local_datetime(&parsed)
                .single()
                .unwrap()
                .with_timezone(&Utc));
        }
        if let Ok(parsed) = NaiveDate::parse_from_str(input, pattern) {
            let offset =
                parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            return Ok(offset
                .from_local_datetime(&parsed.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc));
        }
    }

    Err(format!("Unable to parse date '{}'", input))
}

pub(super) fn datetime_from_parts(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    tz: Option<&str>,
) -> Result<DateTime<Utc>, String> {
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| format!("Invalid date {}-{}-{}", year, month, day))?;
    let naive = date
        .and_hms_milli_opt(hour, minute, second, millis)
        .ok_or_else(|| "Invalid time components".to_string())?;
    let offset = parse_timezone_offset(tz).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    Ok(offset
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| "Could not resolve local datetime".to_string())?
        .with_timezone(&Utc))
}

fn parse_date_value(vm: &dyn BxVM, value: BxValue) -> Result<DateTime<Utc>, String> {
    if value.is_number() || value.is_int() {
        let epoch = Utc
            .timestamp_millis_opt(0)
            .single()
            .ok_or_else(|| "Invalid epoch".to_string())?;
        return Ok(
            epoch + Duration::milliseconds((value.as_number() * 86_400_000.0).round() as i64)
        );
    }
    parse_datetime_input(&vm.to_string(value), None, None)
}

pub(super) fn add_months(dt: DateTime<Utc>, months: i32) -> DateTime<Utc> {
    let total_months = dt.year() * 12 + (dt.month() as i32 - 1) + months;
    let new_year = total_months.div_euclid(12);
    let new_month = total_months.rem_euclid(12) + 1;
    let last_day = match new_month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (new_year % 4 == 0 && new_year % 100 != 0) || (new_year % 400 == 0);
            if leap { 29 } else { 28 }
        }
        _ => 30,
    };
    let day = dt.day().min(last_day);
    let naive = NaiveDate::from_ymd_opt(new_year, new_month as u32, day)
        .unwrap()
        .and_hms_milli_opt(
            dt.hour(),
            dt.minute(),
            dt.second(),
            dt.timestamp_subsec_millis(),
        )
        .unwrap();
    Utc.from_utc_datetime(&naive)
}

fn add_weekdays(mut dt: DateTime<Utc>, amount: f64) -> DateTime<Utc> {
    let mut remaining = amount.round() as i64;
    let step = if remaining < 0 { -1 } else { 1 };
    while remaining != 0 {
        dt += Duration::days(step);
        if !matches!(dt.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
            remaining -= step;
        }
    }
    dt
}

fn date_diff_months(a: DateTime<Utc>, b: DateTime<Utc>) -> i64 {
    let a_total = a.year() as i64 * 12 + a.month() as i64 - 1;
    let b_total = b.year() as i64 * 12 + b.month() as i64 - 1;
    let mut months = a_total - b_total;
    let adjusted = add_months(b, months as i32);
    if months > 0 && adjusted > a {
        months -= 1;
    } else if months < 0 && adjusted < a {
        months += 1;
    }
    months
}

fn date_diff_weekdays(left: DateTime<Utc>, right: DateTime<Utc>) -> i64 {
    let (start, end, sign) = if right <= left {
        (right.date_naive(), left.date_naive(), -1)
    } else {
        (left.date_naive(), right.date_naive(), 1)
    };
    let mut current = start;
    let mut weekdays = 0;
    while current < end {
        if !matches!(
            current.weekday(),
            chrono::Weekday::Sat | chrono::Weekday::Sun
        ) {
            weekdays += 1;
        }
        current += chrono::Duration::days(1);
    }
    sign * weekdays
}

fn now(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let now = Utc::now();
    if let Some(timezone) = args.first().map(|value| vm.to_string(*value)) {
        parse_timezone_offset(Some(&timezone))
            .ok_or_else(|| format!("Unknown timezone '{}'", timezone))?;
        return Ok(BxValue::new_ptr(
            vm.datetime_new_with_timezone(now, &timezone),
        ));
    }
    Ok(BxValue::new_ptr(vm.datetime_new(now)))
}

fn create_date(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("createDate() expects at least 3 arguments: (year, month, day)".to_string());
    }
    let tz = args.get(3).map(|v| vm.to_string(*v));
    let dt = datetime_from_parts(
        args[0].as_number() as i32,
        args[1].as_number() as u32,
        args[2].as_number() as u32,
        0,
        0,
        0,
        0,
        tz.as_deref(),
    )?;
    Ok(BxValue::new_ptr(vm.datetime_new(dt)))
}

fn create_date_time(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 7 {
        return Err("createDateTime() expects at least 7 arguments".to_string());
    }
    let tz = args.get(7).map(|v| vm.to_string(*v));
    let dt = datetime_from_parts(
        args[0].as_number() as i32,
        args[1].as_number() as u32,
        args[2].as_number() as u32,
        args[3].as_number() as u32,
        args[4].as_number() as u32,
        args[5].as_number() as u32,
        args[6].as_number() as u32,
        tz.as_deref(),
    )?;
    Ok(BxValue::new_ptr(vm.datetime_new(dt)))
}

fn date_add(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("dateAdd() expects 3 arguments".to_string());
    }
    let is_member = args[0].as_gc_id().is_some() && !vm.is_string_value(args[0]);
    let (datepart, number, value) = if is_member {
        (vm.to_string(args[1]), args[2].as_number(), args[0])
    } else {
        (vm.to_string(args[0]), args[1].as_number(), args[2])
    };
    let dt = parse_date_value(vm, value)?;
    let result = match datepart.trim().to_ascii_lowercase().as_str() {
        "yyyy" | "yy" | "year" | "years" => add_months(dt, (number.round() as i32) * 12),
        "m" | "month" | "months" => add_months(dt, number.round() as i32),
        "ww" | "week" | "weeks" => dt + Duration::days((number * 7.0).round() as i64),
        "w" => add_weekdays(dt, number),
        "d" | "day" | "days" => dt + Duration::days(number.round() as i64),
        "h" | "hour" | "hours" => dt + Duration::hours(number.round() as i64),
        "n" | "minute" | "minutes" => dt + Duration::minutes(number.round() as i64),
        "l" | "millisecond" | "milliseconds" => dt + Duration::milliseconds(number.round() as i64),
        "s" | "second" | "seconds" => dt + Duration::seconds(number.round() as i64),
        _ => return Err(format!("Unsupported date part '{}'", datepart)),
    };
    Ok(BxValue::new_ptr(vm.datetime_new(result)))
}

fn date_diff(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("dateDiff() expects 3 arguments".to_string());
    }
    let is_member = args[0].as_gc_id().is_some() && !vm.is_string_value(args[0]);
    let (datepart, left, right) = if is_member {
        (vm.to_string(args[1]), args[0], args[2])
    } else {
        (vm.to_string(args[0]), args[1], args[2])
    };
    let left_dt = parse_date_value(vm, left)?;
    let right_dt = parse_date_value(vm, right)?;
    let diff = match datepart.trim().to_ascii_lowercase().as_str() {
        "yyyy" | "yy" | "year" | "years" => (-date_diff_months(left_dt, right_dt) / 12) as f64,
        "m" | "month" | "months" => -date_diff_months(left_dt, right_dt) as f64,
        "ww" | "w" | "week" | "weeks" => {
            (right_dt.signed_duration_since(left_dt).num_days() / 7) as f64
        }
        "wd" | "weekday" | "weekdays" => date_diff_weekdays(left_dt, right_dt) as f64,
        "d" | "day" | "days" => right_dt.signed_duration_since(left_dt).num_days() as f64,
        "h" | "hour" | "hours" => right_dt.signed_duration_since(left_dt).num_hours() as f64,
        "n" | "minute" | "minutes" => right_dt.signed_duration_since(left_dt).num_minutes() as f64,
        "s" | "second" | "seconds" => right_dt.signed_duration_since(left_dt).num_seconds() as f64,
        "l" | "millisecond" | "milliseconds" => {
            right_dt.signed_duration_since(left_dt).num_milliseconds() as f64
        }
        _ => return Err(format!("Unsupported date part '{}'", datepart)),
    };
    Ok(BxValue::new_number(diff))
}

fn to_epoch_millis(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let value = args
        .first()
        .copied()
        .ok_or_else(|| "toEpochMillis() expects a date".to_string())?;
    let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
    Ok(BxValue::new_number(dt.timestamp_millis() as f64))
}

fn time_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("timeFormat() expects at least 1 argument".to_string());
    }
    let value = args[0];
    let format = args
        .get(1)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v));
    let tz = args
        .get(2)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v))
        .or_else(|| vm.datetime_timezone(value))
        .or_else(|| {
            vm.resolve_variable_path("__default_timezone")
                .map(|v| vm.to_string(v))
        });
    let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
    let formatted = format_datetime(dt, format.as_deref(), "hh:mm a", tz.as_deref())?;
    Ok(BxValue::new_ptr(vm.string_new(formatted)))
}

fn offset_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let date = args
        .first()
        .copied()
        .unwrap_or_else(|| BxValue::new_ptr(vm.string_new("1970-01-01T00:00:00Z".to_string())));
    let timezone = args.get(1).copied();
    let mut forwarded = vec![BxValue::new_ptr(vm.string_new("offset".to_string())), date];
    if let Some(timezone) = timezone {
        forwarded.push(timezone);
    }
    math_datetime::time_units(vm, &forwarded)
}

fn get_timezone_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let date = args
        .first()
        .copied()
        .unwrap_or_else(|| BxValue::new_ptr(vm.string_new("1970-01-01T00:00:00Z".to_string())));
    let mut forwarded = vec![
        BxValue::new_ptr(vm.string_new("gettimezone".to_string())),
        date,
    ];
    if let Some(timezone) = args.get(1).copied() {
        forwarded.push(timezone);
    }
    math_datetime::time_units(vm, &forwarded)
}

fn get_numeric_date_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let date = args
        .first()
        .copied()
        .ok_or_else(|| "getNumericDate() expects a date".to_string())?;
    let forwarded = [
        BxValue::new_ptr(vm.string_new("getnumericdate".to_string())),
        date,
    ];
    math_datetime::time_units(vm, &forwarded)
}

fn get_time_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let date = args
        .first()
        .copied()
        .ok_or_else(|| "getTime() expects a date".to_string())?;
    let forwarded = [BxValue::new_ptr(vm.string_new("gettime".to_string())), date];
    math_datetime::time_units(vm, &forwarded)
}

fn date_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("dateFormat() expects at least 1 argument".to_string());
    }
    let value = args[0];
    let format = args
        .get(1)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v));
    let tz = args
        .get(2)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v))
        .or_else(|| vm.datetime_timezone(value))
        .or_else(|| {
            vm.resolve_variable_path("__default_timezone")
                .map(|v| vm.to_string(v))
        });
    let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
    let mut formatted = format_datetime(dt, format.as_deref(), "dd-MMM-yy", tz.as_deref())?;
    if format
        .as_deref()
        .is_some_and(|mask| mask.eq_ignore_ascii_case("mmmm"))
    {
        if let Some(locale) = args
            .get(3)
            .filter(|v| !v.is_null())
            .map(|v| vm.to_string(*v))
        {
            if locale.to_ascii_lowercase().starts_with("de") {
                formatted = match formatted.as_str() {
                    "January" => "Januar",
                    "February" => "Februar",
                    "March" => "März",
                    "April" => "April",
                    "May" => "Mai",
                    "June" => "Juni",
                    "July" => "Juli",
                    "August" => "August",
                    "September" => "September",
                    "October" => "Oktober",
                    "November" => "November",
                    "December" => "Dezember",
                    _ => formatted.as_str(),
                }
                .to_string();
            }
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(formatted)))
}

fn date_time_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("dateTimeFormat() expects at least 1 argument".to_string());
    }
    let value = args[0];
    let format = args
        .get(1)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v));
    let tz = args
        .get(2)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v))
        .or_else(|| vm.datetime_timezone(value))
        .or_else(|| {
            vm.resolve_variable_path("__default_timezone")
                .map(|v| vm.to_string(v))
        });
    let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
    let formatted = format_datetime(dt, format.as_deref(), "dd-MMM-yyyy HH:mm:ss", tz.as_deref())?;
    Ok(BxValue::new_ptr(vm.string_new(formatted)))
}

fn parse_date_time_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("parseDateTime() expects at least 1 argument".to_string());
    }
    let value = vm.to_string(args[0]);
    let format = args
        .get(1)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v));
    let tz = args
        .get(2)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v))
        .or_else(|| {
            vm.resolve_variable_path("__default_timezone")
                .map(|v| vm.to_string(v))
        });
    let locale = args
        .get(3)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v))
        .or_else(|| {
            vm.resolve_variable_path("__default_locale")
                .map(|v| vm.to_string(v))
        })
        .unwrap_or_else(|| "en_US".to_string());
    let locale_lower = locale.to_ascii_lowercase();
    let dt = if format.is_none()
        && locale_lower.starts_with("es")
        && value.to_ascii_lowercase().contains(" de ")
    {
        let words: Vec<&str> = value.split_whitespace().collect();
        let month_names = [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ];
        let month = words
            .get(2)
            .and_then(|name| {
                month_names
                    .iter()
                    .position(|month| month.eq_ignore_ascii_case(name))
            })
            .map(|month| month as u32 + 1)
            .ok_or_else(|| format!("Unable to parse date '{}'", value))?;
        let day = words
            .first()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| format!("Unable to parse date '{}'", value))?;
        let year = words
            .last()
            .and_then(|value| value.parse::<i32>().ok())
            .ok_or_else(|| format!("Unable to parse date '{}'", value))?;
        datetime_from_parts(year, month, day, 0, 0, 0, 0, tz.as_deref())?
    } else if format.is_none() && locale_lower.starts_with("zh") && value.contains('年') {
        let parts: Vec<&str> = value
            .split(['年', '月', '日'])
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() != 3 {
            return Err(format!("Unable to parse date '{}'", value));
        }
        let year = parts[0]
            .parse::<i32>()
            .map_err(|_| format!("Unable to parse date '{}'", value))?;
        let month = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("Unable to parse date '{}'", value))?;
        let day = parts[2]
            .parse::<u32>()
            .map_err(|_| format!("Unable to parse date '{}'", value))?;
        datetime_from_parts(year, month, day, 0, 0, 0, 0, tz.as_deref())?
    } else if format.is_none()
        && value.matches('/').count() == 2
        && (locale_lower.starts_with("en_au") || locale_lower.starts_with("en_gb"))
    {
        let parts: Vec<&str> = value.split('/').collect();
        let day = parts[0]
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Unable to parse date '{}'", value))?;
        let month = parts[1]
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Unable to parse date '{}'", value))?;
        let year = parts[2]
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("Unable to parse date '{}'", value))?;
        datetime_from_parts(year, month, day, 0, 0, 0, 0, tz.as_deref())?
    } else {
        parse_datetime_input(&value, format.as_deref(), tz.as_deref())?
    };
    let object_timezone = args
        .get(2)
        .filter(|v| !v.is_null())
        .map(|v| vm.to_string(*v))
        .or_else(|| {
            value.find(" GMT").and_then(|start| {
                value[start + 1..]
                    .split_whitespace()
                    .next()
                    .map(str::to_string)
            })
        });
    let id = if let Some(timezone) = object_timezone {
        vm.datetime_new_with_timezone(dt, &timezone)
    } else {
        vm.datetime_new(dt)
    };
    Ok(BxValue::new_ptr(id))
}

fn get_tick_count(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    Ok(BxValue::new_number(since_the_epoch.as_millis() as f64))
}

fn sleep(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("sleep() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        vm.sleep(args[0].as_number() as u64);
        watcher::poll_watchers(vm);
        Ok(BxValue::new_null())
    } else {
        Err("sleep() expects a number (milliseconds)".to_string())
    }
}

fn bx_yield(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    vm.yield_fiber();
    Ok(BxValue::new_null())
}

fn run_async(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("runAsync() expects at least 1 argument".to_string());
    }
    let priority = if args.len() >= 2 && args[1].is_number() {
        args[1].as_number() as u8
    } else {
        0
    };
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "No chunk context available".to_string())?;
    vm.spawn_by_value(&args[0], Vec::new(), priority, chunk)
}

fn create_object(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("createObject() expects at least 2 arguments: (type, class)".to_string());
    }
    let obj_type = vm.to_string(args[0]).to_lowercase();
    let class_name = vm.to_string(args[1]);

    match obj_type.as_str() {
        "java" if class_name.eq_ignore_ascii_case("java.io.File") && args.len() == 2 => {
            let id = vm.native_object_new(Rc::new(RefCell::new(JavaClassPlaceholder)));
            Ok(BxValue::new_ptr(id))
        }
        "java" => jni::create_java_object(vm, &class_name, &args[2..]),
        "rust" => vm.construct_native_class(&class_name, &args[2..]),
        "native" => Err("Use 'rust' type for native objects".to_string()),
        _ => Err(format!("Unknown object type: {}", obj_type)),
    }
}

#[derive(Debug)]
struct JavaClassPlaceholder;

impl BxNativeObject for JavaClassPlaceholder {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err(format!(
            "Java class placeholder method '{}' not found",
            name
        ))
    }
}

fn is_null_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(true));
    }
    Ok(BxValue::new_bool(args[0].is_null()))
}

fn null_value_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if !args.is_empty() {
        return Err("nullValue() expects no arguments".to_string());
    }
    Ok(BxValue::new_null())
}

fn is_array_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let is_array = vm.is_array_value(args[0]);
    if !is_array || args.len() < 2 {
        return Ok(BxValue::new_bool(is_array));
    }
    let dimension = vm.to_string(args[1]).parse::<usize>().unwrap_or(0);
    if let Some(id) = args[0].as_gc_id() {
        let key = (vm as *mut dyn BxVM as *mut () as usize, id);
        if let Some(declared) = ARRAY_DIMENSIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "Array dimension metadata lock poisoned".to_string())?
            .get(&key)
            .copied()
        {
            return Ok(BxValue::new_bool(dimension == declared));
        }
    }
    Ok(BxValue::new_bool(
        dimension > 0 && array_depth(vm, args[0]) == dimension,
    ))
}

fn array_depth(vm: &dyn BxVM, value: BxValue) -> usize {
    if !vm.is_array_value(value) {
        return 0;
    }
    let Some(id) = value.as_gc_id() else {
        return 0;
    };
    if vm.array_len(id) == 0 {
        return 1;
    }
    1 + array_depth(vm, vm.array_get(id, 0))
}

fn is_struct_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(vm.is_struct_value(args[0])))
}

fn is_boolean_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let value = args[0];
    let is_bool = value.is_bool()
        || value.is_number()
        || matches!(
            vm.to_string(value).to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "1" | "0"
        );
    Ok(BxValue::new_bool(is_bool))
}

fn is_string_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(vm.is_string_value(args[0])))
}

fn is_date_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let value = args[0];
    let is_date = vm
        .type_name_from_value(value)
        .map(|name| name.eq_ignore_ascii_case("datetime"))
        .unwrap_or(false)
        || parse_datetime_input(&vm.to_string(value), None, None).is_ok();
    Ok(BxValue::new_bool(is_date))
}

fn is_object_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let value = args[0];
    #[cfg(not(target_arch = "wasm32"))]
    let is_query = value
        .as_gc_id()
        .is_some_and(|id| vm.native_object_query_row_count(id).is_some());
    #[cfg(target_arch = "wasm32")]
    let is_query = false;
    let is_object = if value.is_null()
        || value.is_bool()
        || value.is_number()
        || vm.is_string_value(value)
        || vm.is_array_value(value)
        || vm.is_struct_value(value)
        || vm.is_bytes(value)
        || is_query
    {
        false
    } else if let Some(type_name) = vm.type_name_from_value(value) {
        !matches!(
            type_name.to_ascii_lowercase().as_str(),
            "datetime" | "query" | "range"
        )
    } else {
        value.as_gc_id().is_some()
    };
    Ok(BxValue::new_bool(is_object))
}

fn is_instance_of_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Ok(BxValue::new_bool(false));
    }
    let type_name = vm.to_string(args[1]);
    Ok(BxValue::new_bool(
        vm.value_matches_type_name(args[0], &type_name),
    ))
}

fn is_numeric_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let val = &args[0];
    if val.is_number() {
        return Ok(BxValue::new_bool(true));
    }
    if val.is_null() {
        return Ok(BxValue::new_bool(false));
    }
    let mut s = vm.to_string(*val).trim().to_string();
    if let Some(locale) = args
        .get(1)
        .map(|value| vm.to_string(*value).to_ascii_lowercase())
    {
        let comma_decimal = [
            "de_", "fr_", "it_", "es_", "pt_", "da_", "nl_", "el_", "tr_", "ru_", "pl_", "cs_",
            "hu_",
        ]
        .iter()
        .any(|prefix| locale.starts_with(prefix));
        s = s.replace('\u{00a0}', "").replace(' ', "");
        if comma_decimal {
            s = s.replace('.', "").replace(',', ".");
        } else {
            s = s.replace(',', "");
        }
    }
    let is_num = s.parse::<f64>().is_ok() || s.parse::<i64>().is_ok();
    Ok(BxValue::new_bool(is_num))
}

fn is_simple_value_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let val = &args[0];
    let is_simple = val.is_number()
        || val.is_bool()
        || vm.is_string_value(*val)
        || vm
            .type_name_from_value(*val)
            .map(|name| name.eq_ignore_ascii_case("datetime"))
            .unwrap_or(false);
    Ok(BxValue::new_bool(is_simple))
}

fn js_string_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("jsStringFormat() expects at least 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let mut result = String::with_capacity(input.len() + 10);
    for c in input.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\x0c' => result.push_str("\\f"),
            '\t' => result.push_str("\\t"),
            '"' => result.push_str("\\\""),
            '\'' => result.push_str("\\'"),
            _ => result.push(c),
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn l_justify_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("lJustify() expects exactly 2 arguments".to_string());
    }
    let input = vm.to_string(args[0]);
    let length = args[1].as_number() as usize;
    if length == 0 {
        return Err("lJustify() length must be greater than 0".to_string());
    }
    let input_len = input.chars().count();
    if input_len >= length {
        return Ok(BxValue::new_ptr(vm.string_new(input)));
    }
    let padding = length - input_len;
    let mut result = String::with_capacity(length);
    result.push_str(&input);
    for _ in 0..padding {
        result.push(' ');
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn r_justify_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("rJustify() expects exactly 2 arguments".to_string());
    }
    let input = vm.to_string(args[0]);
    let length = args[1].as_number() as usize;
    if length == 0 {
        return Err("rJustify() length must be greater than 0".to_string());
    }
    let input_len = input.chars().count();
    if input_len >= length {
        return Ok(BxValue::new_ptr(vm.string_new(input)));
    }
    let padding = length - input_len;
    let mut result = String::with_capacity(length);
    for _ in 0..padding {
        result.push(' ');
    }
    result.push_str(&input);
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn paragraph_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("paragraphFormat() expects at least 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let lines: Vec<&str> = input.lines().collect();
    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str("<p>");
        result.push_str(line);
        result.push_str("</p>");
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn slugify_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("slugify() expects at least 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let max_length = args.get(1).map(|v| v.as_number() as usize).unwrap_or(0);
    let allow = args.get(2).map(|v| vm.to_string(*v)).unwrap_or_default();

    let slug = input.trim().to_lowercase();
    let slug = slug.split_whitespace().collect::<Vec<_>>().join("-");
    let pattern = format!("[^a-z0-9{}]", allow);
    let re = regex::Regex::new(&pattern).map_err(|e| format!("slugify() invalid regex: {}", e))?;
    let mut slug = re.replace_all(&slug, "-").to_string();
    let multi_dash = regex::Regex::new("-+").unwrap();
    slug = multi_dash.replace_all(&slug, "-").to_string();
    slug = slug.trim_matches('-').to_string();

    if max_length > 0 && slug.len() > max_length {
        slug = slug[..max_length].to_string();
    }

    Ok(BxValue::new_ptr(vm.string_new(slug)))
}

fn wrap_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("wrap() expects at least 2 arguments".to_string());
    }
    let mut input = vm.to_string(args[0]);
    let limit = args[1].as_number() as usize;
    let strip = args.get(2).map(|v| v.as_bool()).unwrap_or(false);

    if limit == 0 {
        return Err("wrap() limit must be greater than 0".to_string());
    }

    if strip {
        input = input.replace('\r', " ").replace('\n', " ");
        let multi_space = regex::Regex::new("\\s+").unwrap();
        input = multi_space.replace_all(&input, " ").to_string();
    }

    let mut result = String::new();
    let mut index = 0;
    let chars: Vec<char> = input.chars().collect();

    while index < chars.len() {
        if index + limit > chars.len() {
            result.extend(&chars[index..]);
            break;
        }

        let mut space_idx = None;
        for i in (0..=limit).rev() {
            if index + i < chars.len() && chars[index + i] == ' ' {
                space_idx = Some(i);
                break;
            }
        }

        if let Some(space_pos) = space_idx {
            if space_pos == 0 {
                result.extend(&chars[index..index + limit]);
                result.push('\n');
                index += limit;
            } else {
                result.extend(&chars[index..index + space_pos]);
                result.push('\n');
                index += space_pos + 1;
            }
        } else {
            result.extend(&chars[index..index + limit]);
            result.push('\n');
            index += limit;
        }
    }

    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn string_bind_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("stringBind() expects exactly 2 arguments".to_string());
    }
    let input = vm.to_string(args[0]);
    let struct_id = args[1]
        .as_gc_id()
        .ok_or("stringBind() second argument must be a struct")?;

    if !vm.is_struct_value(args[1]) {
        return Err("stringBind() second argument must be a struct".to_string());
    }

    let re = regex::Regex::new(r"\$\{([^:}]+)(?::([^}]+))?\}")
        .map_err(|e| format!("stringBind() regex error: {}", e))?;
    let result = re
        .replace_all(&input, |caps: &regex::Captures| {
            let placeholder = &caps[1];
            let default_val = caps.get(2).map(|m| m.as_str());
            let value = vm.struct_get(struct_id, placeholder);
            if value.is_null() {
                if let Some(def) = default_val {
                    def.to_string()
                } else {
                    caps[0].to_string()
                }
            } else {
                vm.to_string(value)
            }
        })
        .to_string();

    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn query_string_to_struct_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryStringToStruct() expects at least 1 argument".to_string());
    }
    let mut target = vm.to_string(args[0]).trim().to_string();
    let delimiter = args
        .get(1)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| "&".to_string());

    if target.is_empty() {
        let id = vm.struct_new();
        return Ok(BxValue::new_ptr(id));
    }

    if target.starts_with('?') {
        target = target[1..].to_string();
    }

    let struct_id = vm.struct_new();
    for pair in target.split(&delimiter) {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.is_empty() {
            continue;
        }
        let key = urlencoding::decode(parts[0].trim())
            .unwrap_or_else(|_| parts[0].trim().into())
            .to_string();
        let value = if parts.len() > 1 {
            urlencoding::decode(parts[1].trim())
                .unwrap_or_else(|_| parts[1].trim().into())
                .to_string()
        } else {
            String::new()
        };
        let value_id = vm.string_new(value);
        vm.struct_set(struct_id, &key, BxValue::new_ptr(value_id));
    }

    Ok(BxValue::new_ptr(struct_id))
}

fn charset_decode_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("charsetDecode() expects at least 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let encoding = args
        .get(1)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| "utf-8".to_string())
        .to_lowercase();

    let bytes = match encoding.as_str() {
        "utf-8" | "utf8" => input.as_bytes().to_vec(),
        "ascii" | "us-ascii" => input.as_bytes().to_vec(),
        "iso-8859-1" | "latin1" | "latin-1" => input.as_bytes().to_vec(),
        _ => {
            return Err(format!(
                "charsetDecode() unsupported encoding: {}",
                encoding
            ));
        }
    };

    let id = vm.bytes_new(bytes);
    Ok(BxValue::new_ptr(id))
}

fn charset_encode_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("charsetEncode() expects at least 1 argument".to_string());
    }
    let bytes = vm
        .to_bytes(args[0])
        .map_err(|_| "charsetEncode() first argument must be binary data".to_string())?;
    let encoding = args
        .get(1)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| "utf-8".to_string())
        .to_lowercase();

    let result = match encoding.as_str() {
        "utf-8" | "utf8" => String::from_utf8_lossy(&bytes).to_string(),
        "ascii" | "us-ascii" => String::from_utf8_lossy(&bytes).to_string(),
        "iso-8859-1" | "latin1" | "latin-1" => String::from_utf8_lossy(&bytes).to_string(),
        _ => {
            return Err(format!(
                "charsetEncode() unsupported encoding: {}",
                encoding
            ));
        }
    };

    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn sql_prettify_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("sqlPrettify() expects at least 1 argument".to_string());
    }
    if args[0].is_null() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }
    let input = vm.to_string(args[0]);
    if input.trim().is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }

    let sql_keywords = [
        "ALTER TABLE",
        "CREATE TABLE",
        "CASE",
        "NULLIF",
        "DELETE",
        "DROP TABLE",
        "FROM",
        "GROUP BY",
        "HAVING",
        "INSERT INTO",
        "LIMIT",
        "ORDER BY",
        "OFFSET",
        "SELECT",
        "UNION",
        "UPDATE",
        "WHERE",
    ];
    let sql_indented_keywords = [
        "FULL JOIN",
        "INNER JOIN",
        "JOIN",
        "LEFT JOIN",
        "OUTER JOIN",
        "LIKE",
        "BETWEEN",
        "IS NULL",
        "IS NOT NULL",
        "EXISTS",
        "DISTINCT",
        "UNION ALL",
        "INTERSECT",
        "MINUS",
        "EXCEPT",
    ];
    let sql_logical_operators = ["AND", "OR", "NOT"];
    let indent = "  ";

    let mut result = input.clone();

    for keyword in &sql_keywords {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(keyword));
        let re = regex::Regex::new(&pattern).unwrap();
        result = re
            .replace_all(&result, format!("\n{}\n{}", keyword.to_uppercase(), indent))
            .to_string();
    }

    for keyword in &sql_indented_keywords {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(keyword));
        let re = regex::Regex::new(&pattern).unwrap();
        result = re
            .replace_all(&result, format!("\n{}{}", indent, keyword.to_uppercase()))
            .to_string();
    }

    for op in &sql_logical_operators {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(op));
        let re = regex::Regex::new(&pattern).unwrap();
        result = re
            .replace_all(&result, format!("{}\n", op.to_uppercase()))
            .to_string();
    }

    let multi_newline = regex::Regex::new(r"\n\s*\n").unwrap();
    result = multi_newline.replace_all(&result, "\n").to_string();
    result = result.trim().to_string();

    Ok(BxValue::new_ptr(vm.string_new(result)))
}

#[cfg(test)]
mod tests {
    use super::{rand, randomize};
    use crate::{types::BxValue, vm::VM};

    #[test]
    fn seeded_random_generators_are_vm_local() {
        let mut first = VM::new();
        let mut second = VM::new();
        let seed = [BxValue::new_int(12345)];

        randomize(&mut first, &seed).unwrap();
        let expected = rand(&mut first, &[]).unwrap();
        randomize(&mut first, &seed).unwrap();
        rand(&mut second, &[]).unwrap();
        let actual = rand(&mut first, &[]).unwrap();

        assert_eq!(actual, expected);
    }
}

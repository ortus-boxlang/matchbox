import java.time.*;
import java.time.format.*;
import java.util.*;
import java.io.*;

/**
 * Generates golden.tsv, the oracle for the dateFormat/timeFormat/dateTimeFormat parity tests.
 * Mirrors ortus.boxlang.runtime.bifs.global.temporal.DateTimeFormat (BoxLang) on the JVM:
 *   java GenGolden.java > golden.tsv
 * Columns: instant, zone, locale, function, mask, result ("ERR <message>" when Java throws).
 */
public class GenGolden {
    static final DateTimeFormatter ISO_NOMILLIS = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ssXXX");
    static final DateTimeFormatter ODBC_DT = new DateTimeFormatterBuilder().appendLiteral("{ts ").optionalStart().appendLiteral('\'').optionalEnd().appendPattern("yyyy-MM-dd HH:mm:ss").optionalStart().appendLiteral('\'').optionalEnd().appendLiteral('}').toFormatter();
    static final DateTimeFormatter ODBC_D = new DateTimeFormatterBuilder().appendLiteral("{d ").optionalStart().appendLiteral('\'').optionalEnd().appendPattern("yyyy-MM-dd").optionalStart().appendLiteral('\'').optionalEnd().appendLiteral('}').toFormatter();
    static final DateTimeFormatter ODBC_T = new DateTimeFormatterBuilder().appendLiteral("{t ").optionalStart().appendLiteral('\'').optionalEnd().appendPattern("HH:mm:ss").optionalStart().appendLiteral('\'').optionalEnd().appendLiteral('}').toFormatter();
    static final DateTimeFormatter JS = DateTimeFormatter.ofPattern("EEE MMM dd yyyy HH:mm:ss 'GMT'Z (zzzz)");
    static final Map<String, DateTimeFormatter> COMMON = new HashMap<>();
    static {
        c("fullDateTime", DateTimeFormatter.ofLocalizedDateTime(FormatStyle.FULL, FormatStyle.FULL));
        c("longDateTime", DateTimeFormatter.ofLocalizedDateTime(FormatStyle.LONG, FormatStyle.LONG));
        c("mediumDateTime", DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.MEDIUM));
        c("shortDateTime", DateTimeFormatter.ofLocalizedDateTime(FormatStyle.SHORT, FormatStyle.SHORT));
        c("ISODateTime", ISO_NOMILLIS);
        c("ISO8601DateTime", DateTimeFormatter.ISO_OFFSET_DATE_TIME);
        c("ODBCDateTime", ODBC_DT);
        c("ODBCDate", ODBC_D);
        c("ODBCTime", ODBC_T);
        c("javascriptDateTime", JS);
        c("fullDate", DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL));
        c("longDate", DateTimeFormatter.ofLocalizedDate(FormatStyle.LONG));
        c("mediumDate", DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM));
        c("shortDate", DateTimeFormatter.ofLocalizedDate(FormatStyle.SHORT));
        c("ISODate", DateTimeFormatter.ISO_DATE);
        c("ISO8601Date", DateTimeFormatter.ISO_DATE);
        c("fullTime", DateTimeFormatter.ofLocalizedTime(FormatStyle.FULL));
        c("longTime", DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG));
        c("mediumTime", DateTimeFormatter.ofLocalizedTime(FormatStyle.MEDIUM));
        c("shortTime", DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT));
        c("ISOTime", DateTimeFormatter.ISO_TIME);
        c("ISO8601Time", DateTimeFormatter.ISO_TIME);
    }
    static void c(String k, DateTimeFormatter f) { COMMON.put(k.toLowerCase(), f); }

    static String sanitize(String s) {
        return s.trim().replaceAll("\u00A0", " ").replaceAll("\u202F", " ").replaceAll(" +", " ");
    }

    static String run(ZonedDateTime ref, String fn, String mask, Locale loc) {
        try {
            String mode = fn.equals("date") ? "Date" : fn.equals("time") ? "Time" : "DateTime";
            if (mask == null) {
                DateTimeFormatter f = fn.equals("date") ? DateTimeFormatter.ofPattern("dd-MMM-yy")
                    : fn.equals("time") ? DateTimeFormatter.ofPattern("hh:mm a")
                    : DateTimeFormatter.ofPattern("dd-MMM-yyyy HH:mm:ss");
                return sanitize(ref.format(f.withLocale(loc)));
            }
            if (mask.equalsIgnoreCase("epoch")) return Long.toString(ref.toEpochSecond());
            if (mask.equalsIgnoreCase("epochms")) return Long.toString(ref.toInstant().toEpochMilli());
            DateTimeFormatter common = COMMON.get((mask.trim() + mode).toLowerCase());
            if (common != null) return sanitize(ref.format(common.withLocale(loc)));
            DateTimeFormatter f = COMMON.get((mask + "DateTime").toLowerCase());
            if (f == null) f = new DateTimeFormatterBuilder().parseLenient().parseCaseInsensitive().appendPattern(mask).toFormatter();
            return sanitize(ref.format(f.withLocale(loc)));
        } catch (Exception e) {
            return "ERR " + e.getMessage();
        }
    }

    static String esc(String s) { return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r"); }

    static void emit(PrintStream out, String instant, String zone, String locale, String fn, String mask) {
        Locale loc = Locale.forLanguageTag(locale.replace('_', '-'));
        ZonedDateTime ref = Instant.parse(instant).atZone(ZoneId.of(zone));
        String r = run(ref, fn, mask, loc);
        out.println(instant + "\t" + zone + "\t" + locale + "\t" + fn + "\t" + (mask == null ? "<null>" : esc(mask)) + "\t" + esc(r));
    }

    public static void main(String[] a) throws Exception {
        PrintStream out = new PrintStream(new FileOutputStream(a.length > 0 ? a[0] : "golden.tsv"), false, "UTF-8");
        String[] dates = {
            "2024-03-05T14:07:09.123456789Z", "2024-02-29T00:00:00Z", "2023-12-31T23:59:59.999Z", "2021-01-01T12:00:00Z",
            "2024-12-30T00:00:00Z", "0001-01-01T00:00:00Z", "0000-01-01T00:00:00Z", "-0044-03-15T12:00:00Z",
            "9999-12-31T23:59:59Z", "+10000-01-01T00:00:00Z", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59.500Z",
            "2024-07-04T12:30:00Z", "2024-03-10T00:05:00Z", "2024-06-15T00:00:00Z", "2023-01-01T00:00:00Z",
            "2026-10-19T09:45:30.500Z", "2024-11-03T01:30:00Z", "2100-02-28T23:59:59Z", "2000-02-29T12:00:00Z"
        };
        LinkedHashSet<String> letterMasks = new LinkedHashSet<>();
        for (char ch = 'A'; ch <= 'z'; ch++) {
            if (!Character.isLetter(ch) || ch > 'z') continue;
            for (int n = 1; n <= 6; n++) letterMasks.add(String.valueOf(ch).repeat(n));
        }
        LinkedHashSet<String> extraMasks = new LinkedHashSet<>(Arrays.asList(
            "'T'", "yyyy-MM-dd'T'HH:mm:ss", "''", "'o''clock'", "h 'o''clock' a", "'abc", "[yyyy]", "yyyy[-MM[-dd]]", "{", "}", "#", "[", "]", "[[yyyy]]",
            "yyyy-MM-dd HH:mm:ss.SSS", "dd-MMM-yy", "EEEE, MMMM d, yyyy", "MM/dd/yyyy", "mm/dd/yyyy", "yyyy-mm-dd", "mmm d, yyyy", "hh:mm:ss tt", "hh:mm:ss a",
            "yyyy-MM-dd'T'HH:mm:ss.SSSXXX", "yyyy-MM-dd'T'HH:mm:ssZ", "yyyy-MM-dd'T'HH:mm:ss.SSSX", "ZZZZZ", "ppH", "pppyyyy", "ppppppppH", "p", "pp",
            "d MMMM yyyy", "YYYY-'W'ww-e", "yyyyMMddHHmmssSSS", "HHmmss", "SSSSSSSSS", "SSSSSSSSSS", "yyyyyyyyyy", "yyyyyyyyyyyyyyyyyyyyyy",
            "", " ", "   ", "MMM  dd", "  yyyy  ", "yyyy MM dd", "'日本'yyyy", "yyyy年MM月dd日", "uuuu-MM-dd", "G yyyy", "GGGG yyyy", "MMMM yyyy",
            "yyyy-DDD", "EEE, dd MMM yyyy HH:mm:ss Z", "hh:mm a z", "HH:mm:ss zzzz", "VV", "xxx", "XXXXX", "OOOO", "BBBB", "yyyy-MM-dd'T'HH:mm:ss.SSSSSSXXX",
            "yyyy-MM-dd\u00A0HH", "yyyy.MM.dd G 'at' HH:mm:ss z", "K:mm a, z", "yyyyy.MMMMM.dd", "QQQ yyyy", "Q/yy", "w", "W", "F", "e", "c",
            "a h", "h:mm a", "h:mm:ss a", "H:m:s", "MMMMM", "LLLLL", "ccccc", "eeeee", "ccc", "eee"
        ));
        LinkedHashSet<String> fullMasks = new LinkedHashSet<>(letterMasks);
        fullMasks.addAll(extraMasks);
        List<String> named = new ArrayList<>(Arrays.asList(
            "short", "medium", "long", "full", "iso", "iso8601", "odbc", "javascript", "epoch", "epochms", "SHORT", "Medium", " long ", "ISO", "Odbc", "EPOCH",
            "shortDate", "fullDateTime", "ISODate", "odbcdate", "ODBCTime", "mediumTime", "longTime", "xyz", "xxx", "iso8601 ", "shortTime", "fullTime", "fullDate", "longDate", "ODBCDateTime"
        ));
        String[] fns = {"date", "time", "datetime"};
        for (int i = 0; i < dates.length; i++) {
            String d = dates[i];
            // every letter at every repeat count for the first dates, a core set for the rest
            for (String m : (i < 8 ? fullMasks : extraMasks)) emit(out, d, "UTC", "en_US", "datetime", m);
            for (String fn : fns) {
                emit(out, d, "UTC", "en_US", fn, null);
                for (String m : named) emit(out, d, "UTC", "en_US", fn, m);
            }
        }
        // fixed offset zones
        for (int i = 0; i < 6; i++) {
            for (String z : new String[]{"+05:30", "-08:00", "+00:00", "Z", "GMT"}) {
                for (String m : (i < 2 ? fullMasks : extraMasks)) emit(out, dates[i], z, "en_US", "datetime", m);
                for (String fn : fns) {
                    emit(out, dates[i], z, "en_US", fn, null);
                    for (String m : named) emit(out, dates[i], z, "en_US", fn, m);
                }
            }
        }
        // locales
        String[] locales = {"en_GB", "de_DE", "es_ES", "fr_FR", "it_IT", "pt_BR", "nl_NL", "ru_RU", "ja_JP", "zh_CN"};
        String[] lmasks = {"MMM", "MMMM", "MMMMM", "LLL", "LLLL", "LLLLL", "EEE", "EEEE", "EEEEE", "ccc", "cccc", "a", "G", "GGGG", "GGGGG", "QQQ", "QQQQ", "QQQQQ", "qqq", "qqqq", "yyyy-MM-dd", "d. MMMM yyyy", "EEEE, d MMMM", "hh:mm a", "BBBB", "w", "e", "ee", "c"};
        for (String loc : locales) {
            for (String m : new String[]{"z", "zzzz", "v", "vvvv", "O", "OOOO", "VV"}) emit(out, dates[0], "GMT", loc, "datetime", m);
            for (int i : new int[]{0, 2, 16, 12, 13}) {
                for (String m : lmasks) emit(out, dates[i], "UTC", loc, "datetime", m);
                for (String fn : fns) {
                    emit(out, dates[i], "UTC", loc, fn, null);
                    for (String m : new String[]{"short", "medium", "long", "full", "iso", "iso8601"}) emit(out, dates[i], "UTC", loc, fn, m);
                }
            }
        }
        out.close();
    }
}

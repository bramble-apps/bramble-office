//! Unit conversion for the Convert sheet (iOS 18): fifteen categories in Apple's order, units
//! listed alphabetically by name. Linear units carry a factor to the category's base unit;
//! temperature is affine and fuel economy mixes distance-per-volume with volume-per-distance.
//! Currency units come from an exchange-rate table (open.er-api.com format, USD base).

use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize)]
pub struct Unit {
    pub id: String,
    pub name: String,
    pub symbol: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub units: Vec<Unit>,
}

#[derive(Clone, Debug)]
pub struct Rates {
    pub base: String,
    /// Units of each currency per one `base`.
    pub rates: HashMap<String, f64>,
    /// Unix seconds.
    pub updated: i64,
    pub source: String,
}

pub const RATES_URL: &str = "https://open.er-api.com/v6/latest/USD";

impl Rates {
    pub fn builtin() -> Rates {
        Rates::from_json(include_str!("../data/rates.json")).expect("bundled rates.json")
    }

    pub fn from_json(s: &str) -> Option<Rates> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        if v.get("result").and_then(|r| r.as_str()).is_some_and(|r| r != "success") {
            return None;
        }
        let base = v.get("base_code")?.as_str()?.to_string();
        let mut rates = HashMap::new();
        for (k, r) in v.get("rates")?.as_object()? {
            if let Some(r) = r.as_f64().filter(|r| r.is_finite() && *r > 0.0) {
                rates.insert(k.clone(), r);
            }
        }
        if rates.is_empty() {
            return None;
        }
        rates.entry(base.clone()).or_insert(1.0);
        let updated = v.get("time_last_update_unix").and_then(|t| t.as_i64()).unwrap_or(0);
        let source = v.get("provider").and_then(|p| p.as_str()).unwrap_or("").to_string();
        Some(Rates { base, rates, updated, source })
    }

    pub fn to_json(&self) -> String {
        let rates: serde_json::Map<String, serde_json::Value> =
            self.rates.iter().map(|(k, v)| (k.clone(), (*v).into())).collect();
        serde_json::json!({
            "result": "success",
            "provider": self.source,
            "time_last_update_unix": self.updated,
            "base_code": self.base,
            "rates": rates,
        })
        .to_string()
    }
}

const CATS: [(&str, &str); 15] = [
    ("angle", "Angle"),
    ("area", "Area"),
    ("currency", "Currency"),
    ("data", "Data"),
    ("energy", "Energy"),
    ("force", "Force"),
    ("fuel", "Fuel"),
    ("length", "Length"),
    ("power", "Power"),
    ("pressure", "Pressure"),
    ("speed", "Speed"),
    ("temperature", "Temperature"),
    ("time", "Time"),
    ("volume", "Volume"),
    ("weight", "Weight"),
];

/// (id, name, symbol, factor to the category's base unit). Temperature and fuel factors are unused.
type Def = (&'static str, &'static str, &'static str, f64);

const ANGLE: &[Def] = &[
    ("deg", "Degrees", "°", 1.0),
    ("rad", "Radians", "rad", 180.0 / std::f64::consts::PI),
    ("grad", "Gradians", "grad", 0.9),
    ("rev", "Revolutions", "rev", 360.0),
    ("arcmin", "Arc Minutes", "′", 1.0 / 60.0),
    ("arcsec", "Arc Seconds", "″", 1.0 / 3600.0),
];

const AREA: &[Def] = &[
    ("m2", "Square Meters", "m²", 1.0),
    ("km2", "Square Kilometers", "km²", 1e6),
    ("cm2", "Square Centimeters", "cm²", 1e-4),
    ("mm2", "Square Millimeters", "mm²", 1e-6),
    ("um2", "Square Micrometers", "µm²", 1e-12),
    ("nm2", "Square Nanometers", "nm²", 1e-18),
    ("Mm2", "Square Megameters", "Mm²", 1e12),
    ("in2", "Square Inches", "in²", 0.00064516),
    ("ft2", "Square Feet", "ft²", 0.09290304),
    ("yd2", "Square Yards", "yd²", 0.83612736),
    ("mi2", "Square Miles", "mi²", 2_589_988.110336),
    ("ac", "Acres", "ac", 4046.8564224),
    ("a", "Ares", "a", 100.0),
    ("ha", "Hectares", "ha", 1e4),
];

const DATA: &[Def] = &[
    ("b", "Bits", "bit", 1.0),
    ("nibble", "Nibbles", "nibble", 4.0),
    ("B", "Bytes", "B", 8.0),
    ("kb", "Kilobits", "kb", 1e3),
    ("Mb", "Megabits", "Mb", 1e6),
    ("Gb", "Gigabits", "Gb", 1e9),
    ("Tb", "Terabits", "Tb", 1e12),
    ("kB", "Kilobytes", "kB", 8e3),
    ("MB", "Megabytes", "MB", 8e6),
    ("GB", "Gigabytes", "GB", 8e9),
    ("TB", "Terabytes", "TB", 8e12),
    ("PB", "Petabytes", "PB", 8e15),
    ("KiB", "Kibibytes", "KiB", 8.0 * 1024.0),
    ("MiB", "Mebibytes", "MiB", 8.0 * 1048576.0),
    ("GiB", "Gibibytes", "GiB", 8.0 * 1073741824.0),
    ("TiB", "Tebibytes", "TiB", 8.0 * 1099511627776.0),
    ("PiB", "Pebibytes", "PiB", 8.0 * 1125899906842624.0),
];

const ENERGY: &[Def] = &[
    ("J", "Joules", "J", 1.0),
    ("kJ", "Kilojoules", "kJ", 1e3),
    ("cal", "Calories", "cal", 4.184),
    ("kcal", "Kilocalories", "kcal", 4184.0),
    ("Wh", "Watt Hours", "Wh", 3600.0),
    ("kWh", "Kilowatt Hours", "kWh", 3.6e6),
    ("eV", "Electronvolts", "eV", 1.602176634e-19),
    ("BTU", "British Thermal Units", "BTU", 1055.05585262),
];

const FORCE: &[Def] = &[
    ("N", "Newtons", "N", 1.0),
    ("kN", "Kilonewtons", "kN", 1e3),
    ("lbf", "Pound-Force", "lbf", 4.4482216152605),
    ("dyn", "Dynes", "dyn", 1e-5),
    ("kgf", "Kilogram-Force", "kgf", 9.80665),
];

/// Factor is the constant k where L/100km = k / value for distance-per-volume units
/// (k = 0 marks L/100km itself).
const FUEL: &[Def] = &[
    ("mpg", "Miles per Gallon", "mpg", 100.0 * 3.785411784 / 1.609344),
    ("mpg_imp", "Miles per Imperial Gallon", "mpg (imp.)", 100.0 * 4.54609 / 1.609344),
    ("L/100km", "Liters per 100 Kilometers", "L/100km", 0.0),
    ("km/L", "Kilometers per Liter", "km/L", 100.0),
];

const LENGTH: &[Def] = &[
    ("pm", "Picometers", "pm", 1e-12),
    ("nm", "Nanometers", "nm", 1e-9),
    ("um", "Micrometers", "µm", 1e-6),
    ("mm", "Millimeters", "mm", 1e-3),
    ("cm", "Centimeters", "cm", 1e-2),
    ("dm", "Decimeters", "dm", 0.1),
    ("m", "Meters", "m", 1.0),
    ("km", "Kilometers", "km", 1e3),
    ("Mm", "Megameters", "Mm", 1e6),
    ("in", "Inches", "in", 0.0254),
    ("ft", "Feet", "ft", 0.3048),
    ("yd", "Yards", "yd", 0.9144),
    ("mi", "Miles", "mi", 1609.344),
    ("smi", "Scandinavian Miles", "smi", 1e4),
    ("nmi", "Nautical Miles", "NM", 1852.0),
    ("ftm", "Fathoms", "ftm", 1.8288),
    ("fur", "Furlongs", "fur", 201.168),
    ("au", "Astronomical Units", "au", 149_597_870_700.0),
    ("ly", "Light Years", "ly", 9.4607304725808e15),
    ("pc", "Parsecs", "pc", 3.085_677_581_491_367e16),
];

const POWER: &[Def] = &[
    ("mW", "Milliwatts", "mW", 1e-3),
    ("W", "Watts", "W", 1.0),
    ("kW", "Kilowatts", "kW", 1e3),
    ("MW", "Megawatts", "MW", 1e6),
    ("GW", "Gigawatts", "GW", 1e9),
    ("hp", "Horsepower", "hp", 745.69987158227),
    ("BTU/h", "BTU per Hour", "BTU/h", 0.29307107017),
];

const PRESSURE: &[Def] = &[
    ("Pa", "Pascals", "Pa", 1.0),
    ("hPa", "Hectopascals", "hPa", 100.0),
    ("kPa", "Kilopascals", "kPa", 1e3),
    ("MPa", "Megapascals", "MPa", 1e6),
    ("bar", "Bars", "bar", 1e5),
    ("mbar", "Millibars", "mbar", 100.0),
    ("atm", "Atmospheres", "atm", 101_325.0),
    ("psi", "Pounds per Square Inch", "psi", 6894.757293168),
    ("mmHg", "Millimeters of Mercury", "mmHg", 133.322387415),
    ("inHg", "Inches of Mercury", "inHg", 3386.388640341),
    ("Torr", "Torr", "Torr", 101_325.0 / 760.0),
];

const SPEED: &[Def] = &[
    ("m/s", "Meters per Second", "m/s", 1.0),
    ("km/h", "Kilometers per Hour", "km/h", 1.0 / 3.6),
    ("mph", "Miles per Hour", "mph", 0.44704),
    ("kn", "Knots", "kn", 1852.0 / 3600.0),
    ("ft/s", "Feet per Second", "ft/s", 0.3048),
];

const TEMPERATURE: &[Def] = &[
    ("C", "Celsius", "°C", 0.0),
    ("F", "Fahrenheit", "°F", 0.0),
    ("K", "Kelvin", "K", 0.0),
];

const DAY: f64 = 86_400.0;
const TIME: &[Def] = &[
    ("ns", "Nanoseconds", "ns", 1e-9),
    ("us", "Microseconds", "μs", 1e-6),
    ("ms", "Milliseconds", "ms", 1e-3),
    ("s", "Seconds", "s", 1.0),
    ("min", "Minutes", "min", 60.0),
    ("h", "Hours", "h", 3600.0),
    ("d", "Days", "d", DAY),
    ("wk", "Weeks", "wk", 7.0 * DAY),
    ("mo", "Months", "mo", 30.4375 * DAY),
    ("yr", "Years", "yr", 365.25 * DAY),
    ("dec", "Decades", "dec", 3652.5 * DAY),
    ("cent", "Centuries", "c", 36525.0 * DAY),
];

const VOLUME: &[Def] = &[
    ("mL", "Milliliters", "mL", 1e-3),
    ("cL", "Centiliters", "cL", 1e-2),
    ("dL", "Deciliters", "dL", 0.1),
    ("L", "Liters", "L", 1.0),
    ("kL", "Kiloliters", "kL", 1e3),
    ("cm3", "Cubic Centimeters", "cm³", 1e-3),
    ("m3", "Cubic Meters", "m³", 1e3),
    ("in3", "Cubic Inches", "in³", 0.016387064),
    ("ft3", "Cubic Feet", "ft³", 28.316846592),
    ("yd3", "Cubic Yards", "yd³", 764.554857984),
    ("acft", "Acre-Feet", "ac⋅ft", 1_233_481.83754752),
    ("gal", "Gallons", "gal", 3.785411784),
    ("qt", "Quarts", "qt", 0.946352946),
    ("pt", "Pints", "pt", 0.473176473),
    ("cup", "Cups", "cup", 0.2365882365),
    ("floz", "Fluid Ounces", "fl oz", 0.0295735295625),
    ("tbsp", "Tablespoons", "tbsp", 0.01478676478125),
    ("tsp", "Teaspoons", "tsp", 0.00492892159375),
    ("gal_imp", "Imperial Gallons", "gal (imp.)", 4.54609),
    ("qt_imp", "Imperial Quarts", "qt (imp.)", 1.1365225),
    ("pt_imp", "Imperial Pints", "pt (imp.)", 0.56826125),
    ("floz_imp", "Imperial Fluid Ounces", "fl oz (imp.)", 0.0284130625),
];

const WEIGHT: &[Def] = &[
    ("ug", "Micrograms", "µg", 1e-9),
    ("mg", "Milligrams", "mg", 1e-6),
    ("g", "Grams", "g", 1e-3),
    ("kg", "Kilograms", "kg", 1.0),
    ("t", "Metric Tons", "t", 1e3),
    ("ct", "Carats", "ct", 2e-4),
    ("oz", "Ounces", "oz", 0.028349523125),
    ("ozt", "Troy Ounces", "oz t", 0.0311034768),
    ("lb", "Pounds", "lb", 0.45359237),
    ("st", "Stones", "st", 6.35029318),
    ("ton", "Short Tons", "ton", 907.18474),
    ("ton_imp", "Long Tons", "LT", 1016.0469088),
    ("slug", "Slugs", "slug", 14.59390294),
];

fn defs(cat: &str) -> Option<&'static [Def]> {
    Some(match cat {
        "angle" => ANGLE,
        "area" => AREA,
        "data" => DATA,
        "energy" => ENERGY,
        "force" => FORCE,
        "fuel" => FUEL,
        "length" => LENGTH,
        "power" => POWER,
        "pressure" => PRESSURE,
        "speed" => SPEED,
        "temperature" => TEMPERATURE,
        "time" => TIME,
        "volume" => VOLUME,
        "weight" => WEIGHT,
        _ => return None,
    })
}

fn factor(cat: &str, id: &str) -> Option<f64> {
    defs(cat)?.iter().find(|d| d.0 == id).map(|d| d.3)
}

pub fn currency_name(code: &str) -> Option<&'static str> {
    CURRENCY_NAMES.iter().find(|c| c.0 == code).map(|c| c.1)
}

pub fn categories(rates: &Rates) -> Vec<Category> {
    CATS.iter().filter_map(|c| category(rates, c.0)).collect()
}

pub fn category(rates: &Rates, id: &str) -> Option<Category> {
    let &(id, name) = CATS.iter().find(|c| c.0 == id)?;
    let mut units: Vec<Unit> = if id == "currency" {
        rates
            .rates
            .keys()
            .map(|code| Unit {
                id: code.clone(),
                name: currency_name(code).unwrap_or(code).to_string(),
                symbol: code.clone(),
            })
            .collect()
    } else {
        defs(id)?
            .iter()
            .map(|d| Unit { id: d.0.into(), name: d.1.into(), symbol: d.2.into() })
            .collect()
    };
    units.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    Some(Category { id, name, units })
}

pub fn unit(rates: &Rates, cat: &str, unit: &str) -> Option<Unit> {
    if cat == "currency" {
        return rates.rates.contains_key(unit).then(|| Unit {
            id: unit.into(),
            name: currency_name(unit).unwrap_or(unit).into(),
            symbol: unit.into(),
        });
    }
    defs(cat)?
        .iter()
        .find(|d| d.0 == unit)
        .map(|d| Unit { id: d.0.into(), name: d.1.into(), symbol: d.2.into() })
}

pub fn convert(cat: &str, from: &str, to: &str, v: f64, rates: &Rates) -> Option<f64> {
    match cat {
        "currency" => Some(v / rates.rates.get(from)? * rates.rates.get(to)?),
        "temperature" => {
            let k = match from {
                "C" => v + 273.15,
                "F" => (v - 32.0) * 5.0 / 9.0 + 273.15,
                "K" => v,
                _ => return None,
            };
            match to {
                "C" => Some(k - 273.15),
                "F" => Some((k - 273.15) * 9.0 / 5.0 + 32.0),
                "K" => Some(k),
                _ => None,
            }
        }
        "fuel" => {
            let (kf, kt) = (factor(cat, from)?, factor(cat, to)?);
            if from == to {
                return Some(v);
            }
            let l100 = if kf == 0.0 { v } else { kf / v };
            Some(if kt == 0.0 { l100 } else { kt / l100 })
        }
        _ => Some(v * factor(cat, from)? / factor(cat, to)?),
    }
}

pub fn default_pair(cat: &str) -> (&'static str, &'static str) {
    match cat {
        "angle" => ("deg", "rad"),
        "area" => ("m2", "ft2"),
        "currency" => ("USD", "EUR"),
        "data" => ("MB", "GB"),
        "energy" => ("kcal", "kJ"),
        "force" => ("N", "lbf"),
        "fuel" => ("mpg", "L/100km"),
        "length" => ("m", "ft"),
        "power" => ("kW", "hp"),
        "pressure" => ("bar", "psi"),
        "speed" => ("km/h", "mph"),
        "temperature" => ("C", "F"),
        "time" => ("h", "min"),
        "volume" => ("L", "gal"),
        "weight" => ("kg", "lb"),
        _ => ("", ""),
    }
}

pub fn search(rates: &Rates, query: &str) -> Vec<(&'static str, Unit)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    categories(rates)
        .into_iter()
        .flat_map(|c| {
            let id = c.id;
            c.units.into_iter().map(move |u| (id, u))
        })
        .filter(|(_, u)| {
            u.name.to_lowercase().contains(&q)
                || u.symbol.to_lowercase().contains(&q)
                || u.id.to_lowercase().contains(&q)
        })
        .collect()
}

const CURRENCY_NAMES: &[(&str, &str)] = &[
    ("AED", "UAE Dirham"),
    ("AFN", "Afghan Afghani"),
    ("ALL", "Albanian Lek"),
    ("AMD", "Armenian Dram"),
    ("ANG", "Netherlands Antillean Guilder"),
    ("AOA", "Angolan Kwanza"),
    ("ARS", "Argentine Peso"),
    ("AUD", "Australian Dollar"),
    ("AWG", "Aruban Florin"),
    ("AZN", "Azerbaijani Manat"),
    ("BAM", "Bosnia-Herzegovina Convertible Mark"),
    ("BBD", "Barbadian Dollar"),
    ("BDT", "Bangladeshi Taka"),
    ("BGN", "Bulgarian Lev"),
    ("BHD", "Bahraini Dinar"),
    ("BIF", "Burundian Franc"),
    ("BMD", "Bermudan Dollar"),
    ("BND", "Brunei Dollar"),
    ("BOB", "Bolivian Boliviano"),
    ("BRL", "Brazilian Real"),
    ("BSD", "Bahamian Dollar"),
    ("BTN", "Bhutanese Ngultrum"),
    ("BWP", "Botswanan Pula"),
    ("BYN", "Belarusian Ruble"),
    ("BZD", "Belize Dollar"),
    ("CAD", "Canadian Dollar"),
    ("CDF", "Congolese Franc"),
    ("CHF", "Swiss Franc"),
    ("CLF", "Chilean Unit of Account (UF)"),
    ("CLP", "Chilean Peso"),
    ("CNH", "Chinese Yuan (Offshore)"),
    ("CNY", "Chinese Yuan"),
    ("COP", "Colombian Peso"),
    ("CRC", "Costa Rican Colón"),
    ("CUP", "Cuban Peso"),
    ("CVE", "Cape Verdean Escudo"),
    ("CZK", "Czech Koruna"),
    ("DJF", "Djiboutian Franc"),
    ("DKK", "Danish Krone"),
    ("DOP", "Dominican Peso"),
    ("DZD", "Algerian Dinar"),
    ("EGP", "Egyptian Pound"),
    ("ERN", "Eritrean Nakfa"),
    ("ETB", "Ethiopian Birr"),
    ("EUR", "Euro"),
    ("FJD", "Fijian Dollar"),
    ("FKP", "Falkland Islands Pound"),
    ("FOK", "Faroese Króna"),
    ("GBP", "British Pound"),
    ("GEL", "Georgian Lari"),
    ("GGP", "Guernsey Pound"),
    ("GHS", "Ghanaian Cedi"),
    ("GIP", "Gibraltar Pound"),
    ("GMD", "Gambian Dalasi"),
    ("GNF", "Guinean Franc"),
    ("GTQ", "Guatemalan Quetzal"),
    ("GYD", "Guyanese Dollar"),
    ("HKD", "Hong Kong Dollar"),
    ("HNL", "Honduran Lempira"),
    ("HRK", "Croatian Kuna"),
    ("HTG", "Haitian Gourde"),
    ("HUF", "Hungarian Forint"),
    ("IDR", "Indonesian Rupiah"),
    ("ILS", "Israeli New Shekel"),
    ("IMP", "Manx Pound"),
    ("INR", "Indian Rupee"),
    ("IQD", "Iraqi Dinar"),
    ("IRR", "Iranian Rial"),
    ("ISK", "Icelandic Króna"),
    ("JEP", "Jersey Pound"),
    ("JMD", "Jamaican Dollar"),
    ("JOD", "Jordanian Dinar"),
    ("JPY", "Japanese Yen"),
    ("KES", "Kenyan Shilling"),
    ("KGS", "Kyrgystani Som"),
    ("KHR", "Cambodian Riel"),
    ("KID", "Kiribati Dollar"),
    ("KMF", "Comorian Franc"),
    ("KRW", "South Korean Won"),
    ("KWD", "Kuwaiti Dinar"),
    ("KYD", "Cayman Islands Dollar"),
    ("KZT", "Kazakhstani Tenge"),
    ("LAK", "Laotian Kip"),
    ("LBP", "Lebanese Pound"),
    ("LKR", "Sri Lankan Rupee"),
    ("LRD", "Liberian Dollar"),
    ("LSL", "Lesotho Loti"),
    ("LYD", "Libyan Dinar"),
    ("MAD", "Moroccan Dirham"),
    ("MDL", "Moldovan Leu"),
    ("MGA", "Malagasy Ariary"),
    ("MKD", "Macedonian Denar"),
    ("MMK", "Myanmar Kyat"),
    ("MNT", "Mongolian Tugrik"),
    ("MOP", "Macanese Pataca"),
    ("MRU", "Mauritanian Ouguiya"),
    ("MUR", "Mauritian Rupee"),
    ("MVR", "Maldivian Rufiyaa"),
    ("MWK", "Malawian Kwacha"),
    ("MXN", "Mexican Peso"),
    ("MYR", "Malaysian Ringgit"),
    ("MZN", "Mozambican Metical"),
    ("NAD", "Namibian Dollar"),
    ("NGN", "Nigerian Naira"),
    ("NIO", "Nicaraguan Córdoba"),
    ("NOK", "Norwegian Krone"),
    ("NPR", "Nepalese Rupee"),
    ("NZD", "New Zealand Dollar"),
    ("OMR", "Omani Rial"),
    ("PAB", "Panamanian Balboa"),
    ("PEN", "Peruvian Sol"),
    ("PGK", "Papua New Guinean Kina"),
    ("PHP", "Philippine Peso"),
    ("PKR", "Pakistani Rupee"),
    ("PLN", "Polish Zloty"),
    ("PYG", "Paraguayan Guarani"),
    ("QAR", "Qatari Riyal"),
    ("RON", "Romanian Leu"),
    ("RSD", "Serbian Dinar"),
    ("RUB", "Russian Ruble"),
    ("RWF", "Rwandan Franc"),
    ("SAR", "Saudi Riyal"),
    ("SBD", "Solomon Islands Dollar"),
    ("SCR", "Seychellois Rupee"),
    ("SDG", "Sudanese Pound"),
    ("SEK", "Swedish Krona"),
    ("SGD", "Singapore Dollar"),
    ("SHP", "St. Helena Pound"),
    ("SLE", "Sierra Leonean Leone"),
    ("SLL", "Sierra Leonean Leone (1964–2022)"),
    ("SOS", "Somali Shilling"),
    ("SRD", "Surinamese Dollar"),
    ("SSP", "South Sudanese Pound"),
    ("STN", "São Tomé & Príncipe Dobra"),
    ("SYP", "Syrian Pound"),
    ("SZL", "Swazi Lilangeni"),
    ("THB", "Thai Baht"),
    ("TJS", "Tajikistani Somoni"),
    ("TMT", "Turkmenistani Manat"),
    ("TND", "Tunisian Dinar"),
    ("TOP", "Tongan Paʻanga"),
    ("TRY", "Turkish Lira"),
    ("TTD", "Trinidad & Tobago Dollar"),
    ("TVD", "Tuvaluan Dollar"),
    ("TWD", "New Taiwan Dollar"),
    ("TZS", "Tanzanian Shilling"),
    ("UAH", "Ukrainian Hryvnia"),
    ("UGX", "Ugandan Shilling"),
    ("USD", "US Dollar"),
    ("UYU", "Uruguayan Peso"),
    ("UZS", "Uzbekistani Som"),
    ("VES", "Venezuelan Bolívar"),
    ("VND", "Vietnamese Dong"),
    ("VUV", "Vanuatu Vatu"),
    ("WST", "Samoan Tala"),
    ("XAF", "Central African CFA Franc"),
    ("XCD", "East Caribbean Dollar"),
    ("XCG", "Caribbean Guilder"),
    ("XDR", "Special Drawing Rights"),
    ("XOF", "West African CFA Franc"),
    ("XPF", "CFP Franc"),
    ("YER", "Yemeni Rial"),
    ("ZAR", "South African Rand"),
    ("ZMW", "Zambian Kwacha"),
    ("ZWG", "Zimbabwean Gold"),
    ("ZWL", "Zimbabwean Dollar (2009)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    fn cv(cat: &str, from: &str, to: &str, v: f64) -> f64 {
        convert(cat, from, to, v, &Rates::builtin()).unwrap()
    }

    #[test]
    fn temperature() {
        assert!(close(cv("temperature", "C", "F", 100.0), 212.0, 1e-12));
        assert!(close(cv("temperature", "C", "K", 0.0), 273.15, 1e-12));
        assert!(close(cv("temperature", "F", "C", -40.0), -40.0, 1e-12));
    }

    #[test]
    fn fuel_inverse() {
        assert!(close(cv("fuel", "L/100km", "mpg", 10.0), 23.5215, 1e-5));
        assert!(close(cv("fuel", "mpg", "L/100km", 23.5215), 10.0, 1e-5));
        assert!(close(cv("fuel", "km/L", "L/100km", 20.0), 5.0, 1e-12));
        assert!(close(cv("fuel", "mpg", "mpg_imp", 1.0), 1.20095, 1e-5));
    }

    #[test]
    fn length_and_data() {
        assert!(close(cv("length", "mi", "m", 1.0), 1609.344, 1e-12));
        assert!(close(cv("length", "ft", "in", 1.0), 12.0, 1e-12));
        assert!(close(cv("data", "GB", "MB", 1.0), 1000.0, 1e-12));
        assert!(close(cv("data", "GiB", "MiB", 1.0), 1024.0, 1e-12));
        assert!(close(cv("data", "B", "b", 1.0), 8.0, 1e-12));
        assert!(close(cv("data", "Mb", "kB", 8.0), 1000.0, 1e-12));
        assert!(close(cv("angle", "rev", "rad", 1.0), std::f64::consts::TAU, 1e-12));
    }

    #[test]
    fn currency() {
        let r = Rates::builtin();
        assert_eq!(r.base, "USD");
        assert!(r.rates.len() > 150);
        assert_eq!(convert("currency", "USD", "USD", 1.0, &r), Some(1.0));
        let eur = convert("currency", "USD", "EUR", 10.0, &r).unwrap();
        assert!(close(convert("currency", "EUR", "USD", eur, &r).unwrap(), 10.0, 1e-12));
        assert_eq!(convert("currency", "USD", "XXX", 1.0, &r), None);
        for code in r.rates.keys() {
            assert!(currency_name(code).is_some(), "no name for {code}");
        }
    }

    #[test]
    fn categories_complete() {
        let r = Rates::builtin();
        let cats = categories(&r);
        assert_eq!(cats.len(), 15);
        assert_eq!(cats[0].id, "angle");
        assert_eq!(cats[14].id, "weight");
        for c in &cats {
            assert!(!c.units.is_empty(), "{}", c.id);
            let names: Vec<_> = c.units.iter().map(|u| u.name.to_lowercase()).collect();
            assert!(names.windows(2).all(|w| w[0] <= w[1]), "{} not sorted", c.id);
            let mut ids: Vec<_> = c.units.iter().map(|u| &u.id).collect();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), c.units.len(), "{} duplicate ids", c.id);
            let (a, b) = default_pair(c.id);
            assert!(unit(&r, c.id, a).is_some() && unit(&r, c.id, b).is_some(), "{}", c.id);
            for u in &c.units {
                assert!(convert(c.id, &u.id, a, 1.0, &r).is_some(), "{} {}", c.id, u.id);
            }
        }
    }

    #[test]
    fn json_round_trip() {
        let r = Rates::builtin();
        let back = Rates::from_json(&r.to_json()).unwrap();
        assert_eq!(back.base, r.base);
        assert_eq!(back.updated, r.updated);
        assert_eq!(back.source, r.source);
        assert_eq!(back.rates, r.rates);
        assert!(Rates::from_json("nope").is_none());
    }

    #[test]
    fn search_finds() {
        let r = Rates::builtin();
        assert!(search(&r, "euro").iter().any(|(c, u)| *c == "currency" && u.id == "EUR"));
        assert!(search(&r, "KM").iter().any(|(c, u)| *c == "length" && u.id == "km"));
        assert!(search(&r, "").is_empty());
    }
}

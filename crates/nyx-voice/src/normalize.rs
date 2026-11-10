//! Text cleanup for speech (pt-BR).

const MAX_CHARS: usize = 400;

const UNITS: [&str; 20] = [
    "zero", "um", "dois", "três", "quatro", "cinco", "seis", "sete", "oito", "nove", "dez", "onze", "doze", "treze",
    "quatorze", "quinze", "dezesseis", "dezessete", "dezoito", "dezenove",
];
const TENS: [&str; 10] =
    ["", "", "vinte", "trinta", "quarenta", "cinquenta", "sessenta", "setenta", "oitenta", "noventa"];
const HUNDREDS: [&str; 10] = [
    "", "cento", "duzentos", "trezentos", "quatrocentos", "quinhentos", "seiscentos", "setecentos", "oitocentos",
    "novecentos",
];

fn below_thousand(n: u64) -> String {
    debug_assert!(n < 1000);
    if n == 100 {
        return "cem".into();
    }
    let mut parts: Vec<String> = vec![];
    let h = n / 100;
    let r = n % 100;
    if h > 0 {
        parts.push(HUNDREDS[h as usize].into());
    }
    if r > 0 {
        if r < 20 {
            parts.push(UNITS[r as usize].into());
        } else {
            let t = TENS[(r / 10) as usize];
            if r % 10 == 0 {
                parts.push(t.into());
            } else {
                parts.push(format!("{} e {}", t, UNITS[(r % 10) as usize]));
            }
        }
    }
    parts.join(" e ")
}

/// Cardinal number in pt-BR (masculine), up to the billions.
pub fn number_words(n: u64) -> String {
    if n == 0 {
        return "zero".into();
    }
    let bi = n / 1_000_000_000;
    let mi = (n / 1_000_000) % 1000;
    let th = (n / 1000) % 1000;
    let rest = n % 1000;
    let mut parts: Vec<String> = vec![];
    if bi > 0 {
        parts.push(if bi == 1 { "um bilhão".into() } else { format!("{} bilhões", below_thousand(bi)) });
    }
    if mi > 0 {
        parts.push(if mi == 1 { "um milhão".into() } else { format!("{} milhões", below_thousand(mi)) });
    }
    if th > 0 {
        parts.push(if th == 1 { "mil".into() } else { format!("{} mil", below_thousand(th)) });
    }
    if rest > 0 {
        parts.push(below_thousand(rest));
    }
    // "mil e quinhentos", "mil duzentos e trinta": "e" only before the last group
    // when it is below 100 or a round hundred.
    let last_val = if rest > 0 {
        rest
    } else if th > 0 {
        th
    } else {
        mi
    };
    let e_before_last = last_val < 100 || last_val % 100 == 0;
    let mut out = String::new();
    let last = parts.len() - 1;
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str(if i == last && e_before_last { " e " } else { " " });
        }
        out.push_str(p);
    }
    out
}

#[derive(Clone, Copy)]
enum Currency {
    Dollar,
    Real,
}

fn read_num(chars: &[char], start: usize) -> (usize, String) {
    let mut j = start;
    while j < chars.len() {
        let c = chars[j];
        if c.is_ascii_digit() {
            j += 1;
        } else if (c == '.' || c == ',') && j + 1 < chars.len() && chars[j + 1].is_ascii_digit() {
            j += 1;
        } else {
            break;
        }
    }
    (j, chars[start..j].iter().collect())
}

fn parse_amount(text: &str) -> Option<(u64, Option<String>)> {
    let (int_s, frac): (String, Option<String>) = if let Some(idx) = text.rfind(',') {
        (text[..idx].replace('.', ""), Some(text[idx + 1..].to_string()))
    } else if let Some(idx) = text.rfind('.') {
        let after = &text[idx + 1..];
        if after.len() == 3 {
            (text.replace('.', ""), None)
        } else {
            (text[..idx].replace('.', ""), Some(after.to_string()))
        }
    } else {
        (text.to_string(), None)
    };
    if int_s.is_empty() || int_s.len() > 12 {
        return None;
    }
    Some((int_s.parse().ok()?, frac))
}

fn money_words(cur: Currency, int: u64, frac: Option<&str>) -> String {
    let mut cents: u64 = 0;
    if let Some(f) = frac {
        let mut f2: String = f.chars().take(2).collect();
        while f2.len() < 2 {
            f2.push('0');
        }
        cents = f2.parse().unwrap_or(0);
    }
    let (one, many) = match cur {
        Currency::Dollar => ("dólar", "dólares"),
        Currency::Real => ("real", "reais"),
    };
    let mut parts = vec![];
    if int > 0 || cents == 0 {
        parts.push(format!("{} {}", number_words(int), if int == 1 { one } else { many }));
    }
    if cents > 0 {
        parts.push(format!("{} {}", number_words(cents), if cents == 1 { "centavo" } else { "centavos" }));
    }
    parts.join(" e ")
}

fn decimal_words(int: u64, frac: &str) -> String {
    let mut out = format!("{} vírgula", number_words(int));
    let digits: String = frac.chars().take(3).collect();
    let trimmed = digits.trim_start_matches('0');
    for _ in 0..(digits.len() - trimmed.len()) {
        out.push_str(" zero");
    }
    if !trimmed.is_empty() {
        out.push(' ');
        out.push_str(&number_words(trimmed.parse().unwrap_or(0)));
    }
    out
}

fn replace_numbers(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let cur = if c == 'U' && chars[i..].starts_with(&['U', 'S', '$']) {
            Some((3, Currency::Dollar))
        } else if c == 'R' && chars[i..].starts_with(&['R', '$']) {
            Some((2, Currency::Real))
        } else if c == '$' {
            Some((1, Currency::Dollar))
        } else {
            None
        };
        let prev_alnum = i > 0 && chars[i - 1].is_alphanumeric();
        if let (Some((len, cur)), false) = (cur, prev_alnum) {
            let mut j = i + len;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if j < chars.len() && chars[j].is_ascii_digit() {
                let (end, text) = read_num(&chars, j);
                if let Some((int, frac)) = parse_amount(&text) {
                    out.push_str(&money_words(cur, int, frac.as_deref()));
                    i = end;
                    continue;
                }
            }
        }
        if c.is_ascii_digit() && !prev_alnum {
            let (end, text) = read_num(&chars, i);
            if end < chars.len() && chars[end] == '%' {
                if let Some((int, frac)) = parse_amount(&text) {
                    match frac {
                        Some(f) => out.push_str(&decimal_words(int, &f)),
                        None => out.push_str(&number_words(int)),
                    }
                    out.push_str(" por cento");
                    i = end + 1;
                    continue;
                }
            }
            out.push_str(&text);
            i = end;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_emoji_or_symbol(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF | 0x2190..=0x21FF
        | 0xFE00..=0xFE0F | 0x200D | 0x20E3 | 0x2500..=0x257F | 0x25A0..=0x25FF)
}

fn strip_links(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']').map(|p| p + i) {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|&c| c == ')').map(|p| p + close) {
                        if out.ends_with('!') {
                            out.pop();
                        }
                        out.extend(&chars[i + 1..close]);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn spell(core: &str) -> Option<&'static str> {
    Some(match core {
        "API" => "á pê i",
        "CLI" => "cê éle i",
        "PR" => "pê érre",
        "MCP" => "ême cê pê",
        "TTS" => "tê tê ésse",
        "SQL" => "ésse quê éle",
        "UI" => "u i",
        "CSV" => "cê ésse vê",
        "URL" => "u érre éle",
        "HTTP" => "agá tê tê pê",
        "SSML" => "ésse ésse ême éle",
        "CPU" => "cê pê u",
        "GPU" => "gê pê u",
        "SDK" => "ésse dê cá",
        "VRM" => "vê érre ême",
        _ => return None,
    })
}

const KNOWN_EXT: [&str; 20] = [
    "rs", "ts", "tsx", "js", "jsx", "json", "md", "toml", "py", "css", "html", "sql", "yml", "yaml", "jsonl", "txt",
    "lock", "ps1", "sh", "log",
];

fn file_ref(core: &str) -> Option<String> {
    let mut base = core;
    let mut nums: Vec<&str> = vec![];
    for _ in 0..2 {
        if let Some((b, n)) = base.rsplit_once(':') {
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                nums.insert(0, n);
                base = b;
                continue;
            }
        }
        break;
    }
    let (name, ext) = base.rsplit_once('.')?;
    if name.is_empty() || ext.is_empty() || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if !ext.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if nums.is_empty() && !KNOWN_EXT.contains(&ext.to_ascii_lowercase().as_str()) {
        return None;
    }
    let name = name.replace(['/', '\\'], " barra ").replace('_', " ").replace('.', " ponto ");
    let mut out = format!("{} ponto {}", name, ext.to_ascii_lowercase());
    if let Some(l) = nums.first() {
        out.push_str(&format!(", linha {}", l));
    }
    if let Some(c) = nums.get(1) {
        out.push_str(&format!(", coluna {}", c));
    }
    Some(out)
}

fn transform_core(core: &str) -> String {
    if core.starts_with("http://") || core.starts_with("https://") {
        return "link".into();
    }
    if let Some(f) = file_ref(core) {
        return f;
    }
    if let Some(s) = spell(core) {
        return s.into();
    }
    if let Some(stem) = core.strip_suffix('s') {
        if let Some(s) = spell(stem) {
            return format!("{}s", s);
        }
    }
    match core {
        "Dev" | "dev" => return "Dév".into(),
        "Devs" | "devs" => return "Dévs".into(),
        _ => {}
    }
    core.replace('_', " ")
}

fn process_token(tok: &str) -> String {
    let lead_len = tok.chars().take_while(|c| "([{\"'“‘".contains(*c)).count();
    let lead: String = tok.chars().take(lead_len).collect();
    let rest: String = tok.chars().skip(lead_len).collect();
    let core_end = rest.trim_end_matches(|c: char| ".,;:!?)]}\"'”’…".contains(c)).len();
    let (core, trail) = rest.split_at(core_end);
    let core = core.trim_matches('_');
    format!("{}{}{}", lead, transform_core(core), trail)
}

fn truncate_sentence(s: &str) -> String {
    if s.chars().count() <= MAX_CHARS {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX_CHARS).collect();
    let end = cut
        .char_indices()
        .filter(|(_, c)| matches!(c, '.' | '!' | '?'))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .filter(|&i| i >= 40);
    let out = match end {
        Some(i) => cut[..i].to_string(),
        None => match cut.rfind(' ') {
            Some(i) => cut[..i].trim_end_matches([',', ';', ':']).to_string(),
            None => cut,
        },
    };
    out.trim().to_string()
}

pub fn normalize(text: &str) -> String {
    let mut lines: Vec<String> = vec![];
    let mut in_fence = false;
    for raw in text.lines() {
        let t = raw.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut l = t.trim_start_matches('>').trim_start().to_string();
        l = l.trim_start_matches('#').trim_start().to_string();
        for b in ["- ", "* ", "+ ", "• "] {
            if let Some(r) = l.strip_prefix(b) {
                l = r.trim_start().to_string();
                break;
            }
        }
        l = strip_links(&l);
        l = l.chars().filter(|c| !is_emoji_or_symbol(*c) && !matches!(c, '*' | '`' | '~' | '|')).collect();
        let l = l.trim().to_string();
        if l.is_empty() || !l.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        let l = if l.ends_with(['.', '!', '?', ':', ';', ',', '…']) { l } else { format!("{}.", l) };
        lines.push(l);
    }
    let joined = replace_numbers(&lines.join(" "));
    let out: Vec<String> = joined.split_whitespace().map(process_token).collect();
    truncate_sentence(&out.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(number_words(3), "três");
        assert_eq!(number_words(41), "quarenta e um");
        assert_eq!(number_words(100), "cem");
        assert_eq!(number_words(1500), "mil e quinhentos");
        assert_eq!(number_words(1230), "mil duzentos e trinta");
        assert_eq!(number_words(2_000_000), "dois milhões");
    }

    #[test]
    fn money_pct_files() {
        assert_eq!(normalize("Custou US$ 3,41 hoje"), "Custou três dólares e quarenta e um centavos hoje.");
        assert_eq!(normalize("R$ 1,00"), "um real.");
        assert_eq!(normalize("Uso em 80% do teto"), "Uso em oitenta por cento do teto.");
        assert_eq!(normalize("12,5%"), "doze vírgula cinco por cento.");
        assert_eq!(normalize("Erro em arquivo.rs:42."), "Erro em arquivo ponto rs, linha 42.");
        assert_eq!(normalize("veja src/main_win.rs:7:3!"), "veja src barra main win ponto rs, linha 7, coluna 3!");
    }

    #[test]
    fn names_and_acronyms() {
        assert_eq!(normalize("Dev terminou"), "Dév terminou.");
        assert_eq!(normalize("Delboni terminou"), "Delboni terminou.");
        assert_eq!(normalize("a API e o CLI"), "a á pê i e o cê éle i.");
        assert_eq!(normalize("abriu o PR no MCP."), "abriu o pê érre no ême cê pê.");
    }

    #[test]
    fn markdown_and_emoji() {
        let t = "## Título\n- **feito** ✅ com `cargo test`\n```\nlet x = 1;\n```\nVeja [docs](https://a.b/c) 🎉";
        assert_eq!(normalize(t), "Título. feito com cargo test. Veja docs.");
        assert_eq!(normalize("https://example.com/x"), "link.");
    }

    #[test]
    fn truncates_on_sentence() {
        let s = "Uma frase curta e completa aqui. ".repeat(30);
        let n = normalize(&s);
        assert!(n.chars().count() <= 400);
        assert!(n.ends_with('.'));
        let long = "palavra ".repeat(200);
        assert!(normalize(&long).chars().count() <= 400);
    }
}

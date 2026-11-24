//! Phone sequence -> cheapest sequence of aliases that actually exist in the bank.
//!
//! Templates describe how a run of phones maps onto units. `req` lists
//! (offset, class) constraints relative to the first consumed phone, so
//! context (the previous vowel) can be matched without being consumed.
//! Anything the bank lacks makes a template non-viable, and the search falls
//! back to a pricier one. New bank families are new template tables.

use crate::{bank::Bank, phones::Phone};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cls {
    Sil,
    V,
    C,
}
use Cls::{C, Sil, V};

struct Template {
    name: &'static str,
    req: &'static [(i32, Cls)],
    consumes: usize,
    emit: &'static [&'static str],
    cost: f32,
}

const SKIP_COST: f32 = 10.0;
const FORCE_END_COST: f32 = 5.0;

/// X-SAMPA CVVC-with-clusters, derived from the Teto English bank's actual aliases:
/// onset clusters are standalone consonant units (`- str`, bare `sk`) followed by a
/// plain CV (`ri`); coda clusters chain as `E l` + `l p-`; `r` uses plain `C C` joins.
/// Every row is only viable if all aliases it emits exist, so rows for conventions a
/// bank doesn't use are harmless. Costs encode preference.
const TEMPLATES: &[Template] = &[
    Template { name: "start_v", req: &[(-1, Sil), (0, V)], consumes: 1, emit: &["- {0}"], cost: 1.0 },
    Template { name: "start_cv", req: &[(-1, Sil), (0, C), (1, V)], consumes: 2, emit: &["- {0}{1}"], cost: 1.0 },
    Template { name: "start_ccv", req: &[(-1, Sil), (0, C), (1, C), (2, V)], consumes: 3, emit: &["- {0}{1}{2}"], cost: 1.0 },
    Template { name: "start_cccv", req: &[(-1, Sil), (0, C), (1, C), (2, C), (3, V)], consumes: 4, emit: &["- {0}{1}{2}{3}"], cost: 1.0 },
    Template { name: "start_cc_cv", req: &[(-1, Sil), (0, C), (1, C), (2, V)], consumes: 3, emit: &["- {0}{1}", "{1}{2}"], cost: 1.2 },
    Template { name: "start_ccc_cv", req: &[(-1, Sil), (0, C), (1, C), (2, C), (3, V)], consumes: 4, emit: &["- {0}{1}{2}", "{2}{3}"], cost: 1.2 },
    Template { name: "start_c_cv", req: &[(-1, Sil), (0, C), (1, C), (2, V)], consumes: 3, emit: &["- {0}", "{1}{2}"], cost: 2.5 },
    Template { name: "vcv", req: &[(-1, V), (0, C), (1, V)], consumes: 2, emit: &["{-1} {0}{1}"], cost: 0.9 },
    Template { name: "vc_cv", req: &[(-1, V), (0, C), (1, V)], consumes: 2, emit: &["{-1} {0}", "{0}{1}"], cost: 1.0 },
    Template { name: "vccv_full", req: &[(-1, V), (0, C), (1, C), (2, V)], consumes: 3, emit: &["{-1} {0}{1}{2}"], cost: 0.9 },
    Template { name: "vc_cl_cv", req: &[(-1, V), (0, C), (1, C), (2, V)], consumes: 3, emit: &["{-1} {0}", "{0}{1}", "{1}{2}"], cost: 1.0 },
    Template { name: "vc_cc_cv", req: &[(-1, V), (0, C), (1, C), (2, V)], consumes: 3, emit: &["{-1} {0}", "{0} {1}", "{1}{2}"], cost: 1.0 },
    Template { name: "vc_ccv", req: &[(-1, V), (0, C), (1, C), (2, V)], consumes: 3, emit: &["{-1} {0}", "{0}{1}{2}"], cost: 1.0 },
    Template { name: "vcc_cv", req: &[(-1, V), (0, C), (1, C), (2, V)], consumes: 3, emit: &["{-1} {0}{1}", "{1}{2}"], cost: 1.1 },
    Template { name: "vc_c_cv", req: &[(-1, V), (0, C), (1, C), (2, V)], consumes: 3, emit: &["{-1} {0}", "{1}{2}"], cost: 1.5 },
    Template { name: "vc_clcl_cv", req: &[(-1, V), (0, C), (1, C), (2, C), (3, V)], consumes: 4, emit: &["{-1} {0}", "{0}{1}{2}", "{2}{3}"], cost: 1.0 },
    Template { name: "vc_cccv", req: &[(-1, V), (0, C), (1, C), (2, C), (3, V)], consumes: 4, emit: &["{-1} {0}", "{0}{1}{2}{3}"], cost: 1.0 },
    Template { name: "vcc_ccv", req: &[(-1, V), (0, C), (1, C), (2, C), (3, V)], consumes: 4, emit: &["{-1} {0}{1}", "{1}{2}{3}"], cost: 1.1 },
    Template { name: "vcc_c_cv", req: &[(-1, V), (0, C), (1, C), (2, C), (3, V)], consumes: 4, emit: &["{-1} {0}{1}", "{2}{3}"], cost: 1.8 },
    Template { name: "vc_end", req: &[(-1, V), (0, C), (1, Sil)], consumes: 1, emit: &["{-1} {0}-"], cost: 1.0 },
    Template { name: "vc_end_plain", req: &[(-1, V), (0, C), (1, Sil)], consumes: 1, emit: &["{-1} {0}"], cost: 2.0 },
    Template { name: "coda2_direct", req: &[(-1, V), (0, C), (1, C), (2, Sil)], consumes: 2, emit: &["{-1} {0}{1}-"], cost: 0.9 },
    Template { name: "coda2_split", req: &[(-1, V), (0, C), (1, C), (2, Sil)], consumes: 2, emit: &["{-1} {0}", "{0} {1}-"], cost: 1.0 },
    Template { name: "coda2_split_plain", req: &[(-1, V), (0, C), (1, C), (2, Sil)], consumes: 2, emit: &["{-1} {0}", "{0} {1}"], cost: 1.3 },
    Template { name: "coda3_direct", req: &[(-1, V), (0, C), (1, C), (2, C), (3, Sil)], consumes: 3, emit: &["{-1} {0}{1}{2}-"], cost: 0.9 },
    Template { name: "coda3_split_a", req: &[(-1, V), (0, C), (1, C), (2, C), (3, Sil)], consumes: 3, emit: &["{-1} {0}", "{0} {1}{2}-"], cost: 1.0 },
    Template { name: "coda3_split_b", req: &[(-1, V), (0, C), (1, C), (2, C), (3, Sil)], consumes: 3, emit: &["{-1} {0}{1}", "{1} {2}-"], cost: 1.1 },
    Template { name: "cc_chain_end", req: &[(-1, C), (0, C), (1, Sil)], consumes: 1, emit: &["{-1} {0}-"], cost: 2.0 },
    Template { name: "end_after_c", req: &[(-1, C), (0, Sil)], consumes: 1, emit: &[], cost: 0.0 },
    Template { name: "vv", req: &[(-1, V), (0, V)], consumes: 1, emit: &["{-1} {0}"], cost: 1.0 },
    Template { name: "v_end", req: &[(-1, V), (0, Sil)], consumes: 1, emit: &["{-1} -"], cost: 1.0 },
    Template { name: "vc_cluster", req: &[(-1, V), (0, C), (1, C)], consumes: 1, emit: &["{-1} {0}"], cost: 1.5 },
    Template { name: "bare_cv", req: &[(0, C), (1, V)], consumes: 2, emit: &["{0}{1}"], cost: 2.5 },
    Template { name: "bare_ccv", req: &[(0, C), (1, C), (2, V)], consumes: 3, emit: &["{0}{1}{2}"], cost: 2.5 },
    Template { name: "bare_v", req: &[(0, V)], consumes: 1, emit: &["{0}"], cost: 4.0 },
];

/// ARPAsing-style phoneme banks: `- X` first, `X` in the middle, `X -` last, plus
/// whatever `X Y` transitions the bank happens to contain (preferred when present).
const ARPA_TEMPLATES: &[Template] = &[
    Template { name: "a_start_v",  req: &[(-1, Sil), (0, V)], consumes: 1, emit: &["- {0}"],      cost: 0.9 },
    Template { name: "a_start_c",  req: &[(-1, Sil), (0, C)], consumes: 1, emit: &["- {0}"],      cost: 0.9 },
    Template { name: "a_vc",       req: &[(-1, V), (0, C)],   consumes: 1, emit: &["{-1} {0}"],   cost: 0.8 },
    Template { name: "a_cv",       req: &[(-1, C), (0, V)],   consumes: 1, emit: &["{-1} {0}"],   cost: 0.8 },
    Template { name: "a_vv",       req: &[(-1, V), (0, V)],   consumes: 1, emit: &["{-1} {0}"],   cost: 0.8 },
    Template { name: "a_cc",       req: &[(-1, C), (0, C)],   consumes: 1, emit: &["{-1} {0}"],   cost: 0.8 },
    Template { name: "a_end_v",    req: &[(0, V), (1, Sil)],  consumes: 1, emit: &["{0} -"],      cost: 0.85 },
    Template { name: "a_end_c",    req: &[(0, C), (1, Sil)],  consumes: 1, emit: &["{0} -"],      cost: 0.85 },
    Template { name: "a_bare_v",   req: &[(0, V)],            consumes: 1, emit: &["{0}"],        cost: 1.0 },
    Template { name: "a_bare_c",   req: &[(0, C)],            consumes: 1, emit: &["{0}"],        cost: 1.0 },
    Template { name: "a_end_sil",  req: &[(0, Sil)],          consumes: 1, emit: &[],             cost: 0.0 },
];

/// What a unit contributes, used to decide how long its "note" lasts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    /// Unit ends in a vowel; carries that vowel's stress (0, 1, 2).
    Vowel(u8),
    /// Consonant-ish transition (VC, bare clusters, ...).
    Cons,
    /// Releases into silence ("V -", "V C-").
    Final,
}

#[derive(Debug, Default, Clone)]
pub struct Plan {
    pub aliases: Vec<String>,
    /// One per alias.
    pub roles: Vec<Role>,
    /// Phones no template could cover (missing from the bank, or inside a cluster).
    pub skipped: Vec<String>,
    /// Template names used, for debugging.
    pub trace: Vec<&'static str>,
    pub cost: f32,
}

#[derive(Clone)]
struct Back {
    from: usize,
    name: &'static str,
    emit: Vec<String>,
    roles: Vec<Role>,
    skipped: Option<String>,
}

fn cls_at(phones: &[Phone], idx: i32) -> Cls {
    if idx < 1 || idx > phones.len() as i32 {
        Cls::Sil
    } else if phones[(idx - 1) as usize].vowel {
        Cls::V
    } else {
        Cls::C
    }
}

fn sym_at(phones: &[Phone], idx: i32) -> &str {
    if idx < 1 || idx > phones.len() as i32 {
        "-"
    } else {
        &phones[(idx - 1) as usize].sym
    }
}

fn fill(pattern: &str, phones: &[Phone], i: i32) -> String {
    // Single pass: "{" is also a real X-SAMPA symbol, so chained str::replace
    // calls could re-match text we just inserted.
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("{-1}") {
            out.push_str(sym_at(phones, i - 1));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{0}") {
            out.push_str(sym_at(phones, i));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{1}") {
            out.push_str(sym_at(phones, i + 1));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{2}") {
            out.push_str(sym_at(phones, i + 2));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{3}") {
            out.push_str(sym_at(phones, i + 3));
            rest = r;
        } else {
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn relax(
    best: &mut [f32],
    back: &mut [Option<Back>],
    from: usize,
    to: usize,
    add: f32,
    name: &'static str,
    emit: Vec<String>,
    roles: Vec<Role>,
    skipped: Option<String>,
) {
    let c = best[from] + add;
    if c < best[to] {
        best[to] = c;
        back[to] = Some(Back { from, name, emit, roles, skipped });
    }
}

fn role_of(pattern: &str, phones: &[Phone], i: i32) -> Role {
    if pattern.ends_with('-') {
        return Role::Final;
    }
    // The last placeholder says what the unit ends on.
    let mut last: Option<i32> = None;
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("{-1}") {
            last = Some(-1);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{0}") {
            last = Some(0);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{1}") {
            last = Some(1);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{2}") {
            last = Some(2);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{3}") {
            last = Some(3);
            rest = r;
        } else {
            let ch = rest.chars().next().unwrap();
            rest = &rest[ch.len_utf8()..];
        }
    }
    match last {
        Some(off) if cls_at(phones, i + off) == Cls::V => {
            Role::Vowel(phones[(i + off - 1) as usize].stress)
        }
        _ => Role::Cons,
    }
}

/// Positions are 1-based over the phones; 0 and n+1 are implicit silence.
pub fn plan(phones: &[Phone], bank: &Bank) -> Plan {
    plan_with(phones, bank, TEMPLATES)
}

/// For ARPAbet-aliased banks; build phones with `phones::arpa_phones`.
pub fn plan_arpa(phones: &[Phone], bank: &Bank) -> Plan {
    plan_with(phones, bank, ARPA_TEMPLATES)
}

fn plan_with(phones: &[Phone], bank: &Bank, templates: &[Template]) -> Plan {
    let n = phones.len();
    if n == 0 {
        return Plan::default();
    }
    let goal = n + 2; // one past the final silence
    let mut best = vec![f32::INFINITY; goal + 1];
    let mut back: Vec<Option<Back>> = vec![None; goal + 1];
    best[1] = 0.0;

    for i in 1..=n + 1 {
        if !best[i].is_finite() {
            continue;
        }
        let ii = i as i32;
        for t in templates {
            if !t.req.iter().all(|&(off, c)| cls_at(phones, ii + off) == c) {
                continue;
            }
            let j = i + t.consumes;
            if j > goal {
                continue;
            }
            let emit: Vec<String> = t.emit.iter().map(|p| fill(p, phones, ii)).collect();
            if !emit.iter().all(|a| bank.has(a)) {
                continue;
            }
            let roles: Vec<Role> = t.emit.iter().map(|p| role_of(p, phones, ii)).collect();
            relax(&mut best, &mut back, i, j, t.cost, t.name, emit, roles, None);
        }
        if i <= n {
            let s = sym_at(phones, ii).to_string();
            relax(&mut best, &mut back, i, i + 1, SKIP_COST, "skip", vec![], vec![], Some(s));
        } else {
            relax(&mut best, &mut back, i, goal, FORCE_END_COST, "force_end", vec![], vec![], None);
        }
    }

    let mut steps = Vec::new();
    let mut j = goal;
    while j > 1 {
        let b = back[j].clone().expect("planner always has a path");
        j = b.from;
        steps.push(b);
    }
    steps.reverse();

    let mut plan = Plan { cost: best[goal], ..Default::default() };
    for b in steps {
        plan.aliases.extend(b.emit);
        plan.roles.extend(b.roles);
        plan.trace.push(b.name);
        if let Some(s) = b.skipped {
            plan.skipped.push(s);
        }
    }
    plan
}

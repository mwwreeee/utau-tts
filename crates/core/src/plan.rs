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

const TEMPLATES: &[Template] = &[
    Template { name: "start_v",   req: &[(-1, Sil), (0, V)],            consumes: 1, emit: &["- {0}"],              cost: 1.0 },
    Template { name: "start_cv",  req: &[(-1, Sil), (0, C), (1, V)],    consumes: 2, emit: &["- {0}{1}"],          cost: 1.0 },
    Template { name: "vcv",       req: &[(-1, V), (0, C), (1, V)],      consumes: 2, emit: &["{-1} {0}{1}"],       cost: 0.9 },
    Template { name: "vc_cv",     req: &[(-1, V), (0, C), (1, V)],      consumes: 2, emit: &["{-1} {0}", "{0}{1}"], cost: 1.0 },
    Template { name: "vc_end",    req: &[(-1, V), (0, C), (1, Sil)],    consumes: 1, emit: &["{-1} {0}-"],         cost: 1.0 },
    Template { name: "vc_end_plain", req: &[(-1, V), (0, C), (1, Sil)], consumes: 1, emit: &["{-1} {0}"],          cost: 2.0 },
    Template { name: "vc_cluster", req: &[(-1, V), (0, C), (1, C)],     consumes: 1, emit: &["{-1} {0}"],          cost: 1.5 },
    Template { name: "vv",        req: &[(-1, V), (0, V)],              consumes: 1, emit: &["{-1} {0}"],          cost: 1.0 },
    Template { name: "v_end",     req: &[(-1, V), (0, Sil)],            consumes: 1, emit: &["{-1} -"],            cost: 1.0 },
    Template { name: "end_after_c", req: &[(-1, C), (0, Sil)],          consumes: 1, emit: &[],                     cost: 0.0 },
    Template { name: "bare_cv",   req: &[(0, C), (1, V)],               consumes: 2, emit: &["{0}{1}"],            cost: 2.5 },
    Template { name: "bare_v",    req: &[(0, V)],                       consumes: 1, emit: &["{0}"],               cost: 4.0 },
];

#[derive(Debug, Default, Clone)]
pub struct Plan {
    pub aliases: Vec<String>,
    pub skipped: Vec<String>,
    pub trace: Vec<&'static str>,
    pub cost: f32,
}

#[derive(Clone)]
struct Back {
    from: usize,
    name: &'static str,
    emit: Vec<String>,
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
    skipped: Option<String>,
) {
    let c = best[from] + add;
    if c < best[to] {
        best[to] = c;
        back[to] = Some(Back { from, name, emit, skipped });
    }
}

pub fn plan(phones: &[Phone], bank: &Bank) -> Plan {
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
        for t in TEMPLATES {
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
            relax(&mut best, &mut back, i, j, t.cost, t.name, emit, None);
        }
        if i <= n {
            let s = sym_at(phones, ii).to_string();
            relax(&mut best, &mut back, i, i + 1, SKIP_COST, "skip", vec![], Some(s));
        } else {
            relax(&mut best, &mut back, i, goal, FORCE_END_COST, "force_end", vec![], None);
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
        plan.trace.push(b.name);
        if let Some(s) = b.skipped {
            plan.skipped.push(s);
        }
    }
    plan
}

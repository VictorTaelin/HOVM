use crate::rulebook as rb;
use crate::dynamic as dn;
use crate::lambolt as lb;
use crate::runtime as rt;

pub fn compile_book(comp: &rb::RuleBook) -> String {
  let mut c_ids = String::new();
  let mut inits = String::new();
  let mut codes = String::new(); 
  for (name, (arity, rules)) in &comp.func_rules {
    let (init, code) = compile_func(comp, &rules, 7);

    line(&mut c_ids, 0, &format!("const u64 {} = {};", name.to_uppercase(), comp.name_to_id.get(name).unwrap_or(&0)));

    line(&mut inits, 6, &format!("case {}: {{", name.to_uppercase()));
    inits.push_str(&init);
    line(&mut inits, 7, &format!("continue;"));
    line(&mut inits, 6, &format!("}};"));

    line(&mut codes, 6, &format!("case {}: {{", name.to_uppercase()));
    codes.push_str(&code);
    line(&mut codes, 7, &format!("break;"));
    line(&mut codes, 6, &format!("}};"));

  }

  return clang_runtime_template(&c_ids, &inits, &codes);
}

pub fn compile_func(comp: &rb::RuleBook, rules: &Vec<lb::Rule>, tab: u64) -> (String, String) {
  let dynfun = dn::build_dynfun(comp, rules);

  let mut init = String::new();
  let mut code = String::new();

  // Computes the initializer, which calls reduce recursivelly
  if dynfun.redex.len() == 0 {
    line(&mut init, tab + 0, &format!("init = 0;"));
  } else {
    line(&mut init, tab + 0, &format!("stk_push(&stack, host);"));
    for (i, x) in dynfun.redex.iter().enumerate() {
      if i < dynfun.redex.len() - 1 && *x {
        line(&mut init, tab + 0, &format!("stk_push(&stack, get_loc(term, {}) | 0x80000000)", i));
      } else {
        line(&mut init, tab + 0, &format!("host = get_loc(term, {});", i));
      }
    }
  }

  // Applies the cal_par rule to superposed args
  for i in 0 .. dynfun.redex.len() as u64 {
    if dynfun.redex[i as usize] {
      line(&mut code, tab + 0, &format!("if (get_tag(ask_arg(mem,term,{})) == PAR) {{", i));
      line(&mut code, tab + 1, &format!("cal_par(mem, host, term, ask_arg(mem, term, {}), {});", i, i));
      line(&mut code, tab + 0, &format!("}}"));
    }
  }

  // For each rule condition vector
  for dynrule in &dynfun.rules {

    let mut matched : Vec<String> = Vec::new();

    // Tests each rule condition (ex: `get_tag(args[0]) == SUCC`)
    for (i, cond) in dynrule.cond.iter().enumerate() {
      let i = i as u64;
      if rt::get_tag(*cond) == rt::U32 {
        let same_tag = format!("get_tag(ask_arg(mem, term, {})) == U32", i);
        let same_val = format!("get_val(ask_arg(mem, term, {})) == {}u", i, rt::get_val(*cond));
        matched.push(format!("({} && {})", same_tag, same_val));
      }
      if rt::get_tag(*cond) == rt::CTR {
        let some_tag = format!("get_tag(ask_arg(mem, term, {})) == CTR", i);
        let some_ext = format!("get_ext(ask_arg(mem, term, {})) == {}u", i, rt::get_ext(*cond));
        matched.push(format!("({} && {})", some_tag, some_ext));
      }
    }

    let conds = if matched.len() == 0 { String::from("1") } else { matched.join(" && ") };
    line(&mut code, tab + 0, &format!("if ({}) {{", conds));

      // Increments the gas count
    line(&mut code, tab + 1, &format!("inc_cost(mem);"));
      
    // Builds the right-hand side term (ex: `(Succ (Add a b))`)
    let done = compile_func_rule_body(&mut code, tab + 1, &dynrule.body, &dynrule.vars);
    line(&mut code, tab + 1, &format!("u64 done = {};", done));

    // Links the host location to it
    line(&mut code, tab + 1, &format!("link(mem, host, done);"));


    // Clears the matched ctrs (the `(Succ ...)` and the `(Add ...)` ctrs)
    line(&mut code, tab + 1, &format!("clear(mem, get_loc(term, 0), {});", dynfun.redex.len()));
    for (i, arity) in &dynrule.free {
      let i = *i as u64;
      line(&mut code, tab + 1, &format!("clear(mem, get_loc(ask_arg(mem, term, {}), 0), {});", i, arity));
    }

    // Collects unused variables (none in this example)
    for (i, dn::DynVar {param, field, erase}) in dynrule.vars.iter().enumerate() {
      if *erase {
        line(&mut code, tab + 1, &format!("collect(mem, {});", get_var(&dynrule.vars[i])));
      }
    }

    line(&mut code, tab + 1, &format!("init = 1;"));
    line(&mut code, tab + 1, &format!("continue;"));

    line(&mut code, tab + 0, &format!("}}"));
  }

  return (init, code);
}

pub fn compile_func_rule_body(code: &mut String, tab: u64, body: &dn::Body, vars: &[dn::DynVar]) -> String {
  let (elem, nodes) = body;
  for i in 0 .. nodes.len() {
    line(code, tab + 0, &format!("u64 loc_{} = alloc(mem, {});", i, nodes[i].len()));
  }
  for i in 0 .. nodes.len() as u64 {
    let node = &nodes[i as usize];
    for j in 0 .. node.len() as u64 {
      match &node[j as usize] {
        dn::Elem::Fix{value} => {
          //mem.node[(host + j) as usize] = *value;
          line(code, tab + 0, &format!("mem->node[loc_{} + {}] = {:#x}u;", i, j, value));
        }
        dn::Elem::Ext{index} => {
          //rt::link(mem, host + j, get_var(mem, term, &vars[*index as usize]));
          line(code, tab + 0, &format!("link(mem, loc_{} + {}, {});", i, j, get_var(&vars[*index as usize])));
          //line(code, tab + 0, &format!("u64 lnk = {};", get_var(&vars[*index as usize])));
          //line(code, tab + 0, &format!("u64 tag = get_tag(lnk);"));
          //line(code, tab + 0, &format!("mem.node[loc_{} + {}] = lnk;", i, j));
          //line(code, tab + 0, &format!("if (tag <= VAR) mem.node[get_loc(lnk, tag & 1)] = Arg(loc_{} + {});", i, j));
        }
        dn::Elem::Loc{value, targ, slot} => {
          //mem.node[(host + j) as usize] = value + hosts[*targ as usize] + slot;
          line(code, tab + 0, &format!("mem->node[loc_{} + {}] = {:#x}u + loc_{} + {};", i, j, value, targ, slot));
        }
      }
    }
  }
  match elem {
    dn::Elem::Fix{value} => format!("{}u", value),
    dn::Elem::Ext{index} => get_var(&vars[*index as usize]),
    dn::Elem::Loc{value, targ, slot} => format!("({}u + loc_{} + {})", value, targ, slot),
  }
}

fn get_var(var: &dn::DynVar) -> String {
  let dn::DynVar {param, field, erase} = var;
  match field {
    Some(i) => { format!("ask_arg(mem, ask_arg(mem, term, {}), {})", param, i) }
    None    => { format!("ask_arg(mem, term, {})", param) }
  }
}

fn line(code: &mut String, tab: u64, line: &str) {
  for i in 0 .. tab {
    code.push_str("  ");
  }
  code.push_str(line);
  code.push('\n');
}

pub fn compile_code(code: &str) -> String {
  let file = lb::read_file(code);
  let book = rb::gen_rulebook(&file);
  let funs = dn::build_runtime_functions(&book);
  return compile_book(&book);
}

pub fn clang_runtime_template(c_ids: &str, inits: &str, codes: &str) -> String {
  return format!(r#"
#include <stdlib.h>
#include <stdio.h>
#include <pthread.h>

#define PARALLEL

#define LIKELY(x) __builtin_expect((x), 1)
#define UNLIKELY(x) __builtin_expect((x), 0)

// Types
// -----

typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long int u64;
typedef pthread_t Thd;

// Consts
// ------

const u64 U64_PER_KB = 0x80;
const u64 U64_PER_MB = 0x20000;
const u64 U64_PER_GB = 0x8000000;

#ifdef PARALLEL
const u64 MAX_WORKERS = 8;
#else
const u64 MAX_WORKERS = 1;
#endif
const u64 MAX_DYNFUNS = 65536;
const u64 MAX_ARITY = 16;
const u64 MEM_SPACE = U64_PER_GB / 2;

// Terms
// -----

typedef u64 Lnk;

const u64 VAL = 1;
const u64 EXT = 0x100000000; 
const u64 ARI = 0x100000000000000;
const u64 TAG = 0x1000000000000000;

const u64 DP0 = 0x0;
const u64 DP1 = 0x1;
const u64 VAR = 0x2;
const u64 ARG = 0x3;
const u64 ERA = 0x4;
const u64 LAM = 0x5;
const u64 APP = 0x6;
const u64 PAR = 0x7;
const u64 CTR = 0x8;
const u64 FUN = 0x9;
const u64 OP2 = 0xA;
const u64 U32 = 0xB;
const u64 F32 = 0xC;
const u64 OUT = 0xE;
const u64 NIL = 0xF;

const u64 ADD = 0x0;
const u64 SUB = 0x1;
const u64 MUL = 0x2;
const u64 DIV = 0x3;
const u64 MOD = 0x4;
const u64 AND = 0x5;
const u64 OR  = 0x6;
const u64 XOR = 0x7;
const u64 SHL = 0x8;
const u64 SHR = 0x9;
const u64 LTN = 0xA;
const u64 LTE = 0xB;
const u64 EQL = 0xC;
const u64 GTE = 0xD;
const u64 GTN = 0xE;
const u64 NEQ = 0xF;

//GENERATED_CONSTRUCTOR_IDS_START//
{}
//GENERATED_CONSTRUCTOR_IDS_END//

// Threads
// -------

typedef struct {{
  u64 size;
  u64* data;
}} Arr;

typedef struct {{
  u64* data;
  u64  size;
  u64  mcap;
}} Stk;

typedef struct {{
  u64  tid;
  Lnk* node;
  u64  size;
  Stk  free[MAX_ARITY];
  u64  cost;

  #ifdef PARALLEL
  u64             has_work;
  pthread_mutex_t has_work_mutex;
  pthread_cond_t  has_work_signal;

  u64             has_result;
  pthread_mutex_t has_result_mutex;
  pthread_cond_t  has_result_signal;

  Thd  thread;
  #endif
}} Worker;

// Globals
// -------

Worker workers[MAX_WORKERS];

const u64 seen_size = 4194304; // uses 32 MB, covers heaps up to 2 GB
u64 seen_data[seen_size]; 

// Array
// -----

void array_write(Arr* arr, u64 idx, u64 value) {{
  arr->data[idx] = value;
}}

u64 array_read(Arr* arr, u64 idx) {{
  return arr->data[idx];
}}

// Stack
// -----

u64 stk_growth_factor = 16;

void stk_init(Stk* stack) {{
  stack->size = 0;
  stack->mcap = stk_growth_factor;
  stack->data = malloc(stack->mcap * sizeof(u64));
}}

void stk_free(Stk* stack) {{
  free(stack->data);
}}

void stk_push(Stk* stack, u64 val) {{
  if (UNLIKELY(stack->size == stack->mcap)) {{ 
    stack->mcap = stack->mcap * stk_growth_factor;
    stack->data = realloc(stack->data, stack->mcap * sizeof(u64));
  }}
  stack->data[stack->size++] = val;
}}

u64 stk_pop(Stk* stack) {{
  if (LIKELY(stack->size > 0)) {{
    // TODO: shrink? -- impacts performance considerably
    //if (stack->size == stack->mcap / stk_growth_factor) {{
      //stack->mcap = stack->mcap / stk_growth_factor;
      //stack->data = realloc(stack->data, stack->mcap * sizeof(u64));
      //printf("shrink %llu\n", stack->mcap);
    //}}
    return stack->data[--stack->size];
  }} else {{
    return -1;
  }}
}}

// Memory
// ------

Lnk Var(u64 pos) {{
  return (VAR * TAG) | pos;
}}

Lnk Dp0(u64 col, u64 pos) {{
  return (DP0 * TAG) | (col * EXT) | pos;
}}

Lnk Dp1(u64 col, u64 pos) {{
  return (DP1 * TAG) | (col * EXT) | pos;
}}

Lnk Arg(u64 pos) {{
  return (ARG * TAG) | pos;
}}

Lnk Era() {{
  return (ERA * TAG);
}}

Lnk Lam(u64 pos) {{
  return (LAM * TAG) | pos;
}}

Lnk App(u64 pos) {{
  return (APP * TAG) | pos;
}}

Lnk Par(u64 col, u64 pos) {{
  return (PAR * TAG) | (col * EXT) | pos;
}}

Lnk Op2(u64 ope, u64 pos) {{
  return (OP2 * TAG) | (ope * EXT) | pos;
}}

Lnk U_32(u64 val) {{
  return (U32 * TAG) | val;
}}

Lnk Nil() {{
  return NIL * TAG;
}}

Lnk Ctr(u64 ari, u64 fun, u64 pos) {{
  return (CTR * TAG) | (ari * ARI) | (fun * EXT) | pos;
}}

Lnk Cal(u64 ari, u64 fun, u64 pos) {{ 
  return (FUN * TAG) | (ari * ARI) | (fun * EXT) | pos;
}}

Lnk Out(u64 arg, u64 fld) {{
  return (OUT * TAG) | (arg << 8) | fld;
}}

u64 get_tag(Lnk lnk) {{
  return lnk / TAG;
}}

u64 get_ext(Lnk lnk) {{
  return (lnk / EXT) & 0xFFFFFF;
}}

u64 get_val(Lnk lnk) {{
  return lnk & 0xFFFFFFFF;
}}

u64 get_ari(Lnk lnk) {{
  return (lnk / ARI) & 0xF;
}}

u64 get_loc(Lnk lnk, u64 arg) {{
  return get_val(lnk) + arg;
}}

Lnk ask_lnk(Worker* mem, u64 loc) {{
  return mem->node[loc];
}}

Lnk ask_arg(Worker* mem, Lnk term, u64 arg) {{
  return ask_lnk(mem, get_loc(term, arg));
}}

u64 link(Worker* mem, u64 loc, Lnk lnk) {{
  mem->node[loc] = lnk;
  //array_write(mem->nodes, loc, lnk);
  if (get_tag(lnk) <= VAR) {{
    mem->node[get_loc(lnk, get_tag(lnk) == DP1 ? 1 : 0)] = Arg(loc);
    //array_write(mem->nodes, get_loc(lnk, get_tag(lnk) == DP1 ? 1 : 0), Arg(loc));
  }}
  return lnk;
}}

u64 alloc(Worker* mem, u64 size) {{
  if (UNLIKELY(size == 0)) {{
    return 0;
  }} else {{
    if (size < 16) {{
      u64 reuse = stk_pop(&mem->free[size]);
      if (reuse != -1) {{
        return reuse;
      }}
    }}
    u64 loc = mem->size;
    mem->size += size;
    return mem->tid * MEM_SPACE + loc;
    //return __atomic_fetch_add(&mem->nodes->size, size, __ATOMIC_RELAXED);
  }}
}}

void clear(Worker* mem, u64 loc, u64 size) {{
  stk_push(&mem->free[size], loc);
}}

// Debug
// -----

void debug_print_lnk(Lnk x) {{
  u64 tag = get_tag(x);
  u64 ext = get_ext(x);
  u64 val = get_val(x);
  switch (tag) {{
    case DP0: printf("DP0"); break;
    case DP1: printf("DP1"); break;
    case VAR: printf("VAR"); break;
    case ARG: printf("ARG"); break;
    case ERA: printf("ERA"); break;
    case LAM: printf("LAM"); break;
    case APP: printf("APP"); break;
    case PAR: printf("PAR"); break;
    case CTR: printf("CTR"); break;
    case FUN: printf("FUN"); break;
    case OP2: printf("OP2"); break;
    case U32: printf("U32"); break;
    case F32: printf("F32"); break;
    case OUT: printf("OUT"); break;
    case NIL: printf("NIL"); break;
    default : printf("???"); break;
  }}
  printf(":%llx:%llx", ext, val);
}}

// Garbage Collection
// ------------------

void collect(Worker* mem, Lnk term) {{
  switch (get_tag(term)) {{
    case DP0: {{
      link(mem, get_loc(term,0), Era());
      //reduce(mem, get_loc(ask_arg(mem,term,1),0));
      break;
    }}
    case DP1: {{
      link(mem, get_loc(term,1), Era());
      //reduce(mem, get_loc(ask_arg(mem,term,0),0));
      break;
    }}
    case VAR: {{
      link(mem, get_loc(term,0), Era());
      break;
    }}
    case LAM: {{
      if (get_tag(ask_arg(mem,term,0)) != ERA) {{
        link(mem, get_loc(ask_arg(mem,term,0),0), Era());
      }}
      collect(mem, ask_arg(mem,term,1));
      clear(mem, get_loc(term,0), 2);
      break;
    }}
    case APP: {{
      collect(mem, ask_arg(mem,term,0));
      collect(mem, ask_arg(mem,term,1));
      clear(mem, get_loc(term,0), 2);
      break;
    }}
    case PAR: {{
      collect(mem, ask_arg(mem,term,0));
      collect(mem, ask_arg(mem,term,1));
      clear(mem, get_loc(term,0), 2);
      break;
    }}
    case OP2: {{
      collect(mem, ask_arg(mem,term,0));
      collect(mem, ask_arg(mem,term,1));
      break;
    }}
    case U32: {{
      break;
    }}
    case CTR: case FUN: {{
      u64 arity = get_ari(term);
      for (u64 i = 0; i < arity; ++i) {{
        collect(mem, ask_arg(mem,term,i));
      }}
      clear(mem, get_loc(term,0), arity);
      break;
    }}
  }}
}}

// Terms
// -----

void inc_cost(Worker* mem) {{
  mem->cost++;
}}

void subst(Worker* mem, Lnk lnk, Lnk val) {{
  if (get_tag(lnk) != ERA) {{
    link(mem, get_loc(lnk,0), val);
  }} else {{
    collect(mem, val);
  }}
}}

Lnk cal_par(Worker* mem, u64 host, Lnk term, Lnk argn, u64 n) {{
  inc_cost(mem);
  u64 arit = get_ari(term);
  u64 func = get_ext(term);
  u64 fun0 = get_loc(term, 0);
  u64 fun1 = alloc(mem, arit);
  u64 par0 = get_loc(argn, 0);
  for (u64 i = 0; i < arit; ++i) {{
    if (i != n) {{
      u64 leti = alloc(mem, 3);
      u64 argi = ask_arg(mem, term, i);
      link(mem, fun0+i, Dp0(get_ext(argn), leti));
      link(mem, fun1+i, Dp1(get_ext(argn), leti));
      link(mem, leti+2, argi);
    }} else {{
      link(mem, fun0+i, ask_arg(mem, argn, 0));
      link(mem, fun1+i, ask_arg(mem, argn, 1));
    }}
  }}
  link(mem, par0+0, Cal(arit, func, fun0));
  link(mem, par0+1, Cal(arit, func, fun1));
  u64 done = Par(get_ext(argn), par0);
  link(mem, host, done);
  return done;
}}

Lnk reduce(Worker* mem, u64 root, u64 depth) {{
  Stk stack;
  stk_init(&stack);

  //u32* stack = mem->stack;

  u64 init = 1;
  u64 size = 1;
  u32 host = (u32)root;

  while (1) {{

    u64 term = ask_lnk(mem, host);

    //printf("reducing: host=%d size=%llu init=%llu ", host, size, init); debug_print_lnk(term); printf("\n");
    //for (u64 i = 0; i < size; ++i) {{
      //printf("- %llu ", stack[i]); debug_print_lnk(ask_lnk(mem, stack[i]>>1)); printf("\n");
    //}}
    
    if (init == 1) {{
      switch (get_tag(term)) {{
        case APP: {{
          stk_push(&stack, host);
          //stack[size++] = host;
          init = 1;
          host = get_loc(term, 0);
          continue;
        }}
        case DP0:
        case DP1: {{
          stk_push(&stack, host);
          //stack[size++] = host;
          host = get_loc(term, 2);
          continue;
        }}
        case OP2: {{
          stk_push(&stack, host);
          stk_push(&stack, get_loc(term, 0) | 0x80000000);
          //stack[size++] = host;
          //stack[size++] = get_loc(term, 0) | 0x80000000;
          host = get_loc(term, 1);
          continue;
        }}
        case FUN: {{
          u64 fun = get_ext(term);
          u64 ari = get_ari(term);
          
          switch (fun)
          //GENERATED_REWRITE_RULES_STEP_0_START//
          {{
{}
          }}
          //GENERATED_REWRITE_RULES_STEP_0_END//

          break;
        }}
      }}

    }} else {{

      switch (get_tag(term)) {{
        case APP: {{
          u64 arg0 = ask_arg(mem, term, 0);
          switch (get_tag(arg0)) {{
            case LAM: {{
              inc_cost(mem);
              subst(mem, ask_arg(mem, arg0, 0), ask_arg(mem, term, 1));
              u64 done = link(mem, host, ask_arg(mem, arg0, 1));
              clear(mem, get_loc(term,0), 2);
              clear(mem, get_loc(arg0,0), 2);
              init = 1;
              continue;
            }}
            case PAR: {{
              inc_cost(mem);
              u64 app0 = get_loc(term, 0);
              u64 app1 = get_loc(arg0, 0);
              u64 let0 = alloc(mem, 3);
              u64 par0 = alloc(mem, 2);
              link(mem, let0+2, ask_arg(mem, term, 1));
              link(mem, app0+1, Dp0(get_ext(arg0), let0));
              link(mem, app0+0, ask_arg(mem, arg0, 0));
              link(mem, app1+0, ask_arg(mem, arg0, 1));
              link(mem, app1+1, Dp1(get_ext(arg0), let0));
              link(mem, par0+0, App(app0));
              link(mem, par0+1, App(app1));
              u64 done = Par(get_ext(arg0), par0);
              link(mem, host, done);
              break;
            }}
          }}
          break;
        }}
        case DP0:
        case DP1: {{
          u64 arg0 = ask_arg(mem, term, 2);
          switch (get_tag(arg0)) {{
            case LAM: {{
              inc_cost(mem);
              u64 let0 = get_loc(term, 0);
              u64 par0 = get_loc(arg0, 0);
              u64 lam0 = alloc(mem, 2);
              u64 lam1 = alloc(mem, 2);
              link(mem, let0+2, ask_arg(mem, arg0, 1));
              link(mem, par0+1, Var(lam1));
              u64 arg0_arg_0 = ask_arg(mem, arg0, 0);
              link(mem, par0+0, Var(lam0));
              subst(mem, arg0_arg_0, Par(get_ext(term), par0));
              u64 term_arg_0 = ask_arg(mem,term,0);
              link(mem, lam0+1, Dp0(get_ext(term), let0));
              subst(mem, term_arg_0, Lam(lam0));
              u64 term_arg_1 = ask_arg(mem,term,1);                      
              link(mem, lam1+1, Dp1(get_ext(term), let0));
              subst(mem, term_arg_1, Lam(lam1));
              u64 done = Lam(get_tag(term) == DP0 ? lam0 : lam1);
              link(mem, host, done);
              init = 1;
              continue;
            }}
            case PAR: {{
              if (get_ext(term) == get_ext(arg0)) {{
                inc_cost(mem);
                subst(mem, ask_arg(mem,term,0), ask_arg(mem,arg0,0));
                subst(mem, ask_arg(mem,term,1), ask_arg(mem,arg0,1));
                u64 done = link(mem, host, ask_arg(mem, arg0, get_tag(term) == DP0 ? 0 : 1));
                clear(mem, get_loc(term,0), 3);
                clear(mem, get_loc(arg0,0), 2);
                init = 1;
                continue;
              }} else {{
                inc_cost(mem);
                u64 par0 = alloc(mem, 2);
                u64 let0 = get_loc(term,0);
                u64 par1 = get_loc(arg0,0);
                u64 let1 = alloc(mem, 3);
                link(mem, let0+2, ask_arg(mem,arg0,0));
                link(mem, let1+2, ask_arg(mem,arg0,1));
                u64 term_arg_0 = ask_arg(mem,term,0);
                u64 term_arg_1 = ask_arg(mem,term,1);
                link(mem, par1+0, Dp1(get_ext(term),let0));
                link(mem, par1+1, Dp1(get_ext(term),let1));
                link(mem, par0+0, Dp0(get_ext(term),let0));
                link(mem, par0+1, Dp0(get_ext(term),let1));
                subst(mem, term_arg_0, Par(get_ext(arg0),par0));
                subst(mem, term_arg_1, Par(get_ext(arg0),par1));
                u64 done = Par(get_ext(arg0), get_tag(term) == DP0 ? par0 : par1);
                link(mem, host, done);
                break;
              }}
            }}
          }}
          if (get_tag(arg0) == U32) {{
            inc_cost(mem);
            subst(mem, ask_arg(mem,term,0), arg0);
            subst(mem, ask_arg(mem,term,1), arg0);
            u64 done = arg0;
            link(mem, host, arg0);
            break;
          }}
          if (get_tag(arg0) == CTR) {{
            inc_cost(mem);
            u64 func = get_ext(arg0);
            u64 arit = get_ari(arg0);
            if (arit == 0) {{
              subst(mem, ask_arg(mem,term,0), Ctr(0, func, 0));
              subst(mem, ask_arg(mem,term,1), Ctr(0, func, 0));
              clear(mem, get_loc(term,0), 3);
              u64 done = link(mem, host, Ctr(0, func, 0));
            }} else {{
              u64 ctr0 = get_loc(arg0,0);
              u64 ctr1 = alloc(mem, arit);
              u64 term_arg_0 = ask_arg(mem,term,0);
              u64 term_arg_1 = ask_arg(mem,term,1);
              for (u64 i = 0; i < arit; ++i) {{
                u64 leti = i == 0 ? get_loc(term,0) : alloc(mem, 3);
                u64 arg0_arg_i = ask_arg(mem, arg0, i);
                link(mem, ctr0+i, Dp0(get_ext(term), leti));
                link(mem, ctr1+i, Dp1(get_ext(term), leti));
                link(mem, leti+2, arg0_arg_i);
              }}
              subst(mem, term_arg_0, Ctr(arit, func, ctr0));
              subst(mem, term_arg_1, Ctr(arit, func, ctr1));
              u64 done = Ctr(arit, func, get_tag(term) == DP0 ? ctr0 : ctr1);
              link(mem, host, done);
            }}
            break;
          }}
          break;
        }}
        case OP2: {{
          u64 arg0 = ask_arg(mem, term, 0);
          u64 arg1 = ask_arg(mem, term, 1);
          if (get_tag(arg0) == U32 && get_tag(arg1) == U32) {{
            inc_cost(mem);
            u64 a = get_val(arg0);
            u64 b = get_val(arg1);
            u64 c = 0;
            switch (get_ext(term)) {{
              case ADD: c = (a +  b) & 0xFFFFFFFF; break;
              case SUB: c = (a -  b) & 0xFFFFFFFF; break;
              case MUL: c = (a *  b) & 0xFFFFFFFF; break;
              case DIV: c = (a /  b) & 0xFFFFFFFF; break;
              case MOD: c = (a %  b) & 0xFFFFFFFF; break;
              case AND: c = (a &  b) & 0xFFFFFFFF; break;
              case OR : c = (a |  b) & 0xFFFFFFFF; break;
              case XOR: c = (a ^  b) & 0xFFFFFFFF; break;
              case SHL: c = (a << b) & 0xFFFFFFFF; break;
              case SHR: c = (a >> b) & 0xFFFFFFFF; break;
              case LTN: c = (a <  b) ? 1 : 0;      break;
              case LTE: c = (a <= b) ? 1 : 0;      break;
              case EQL: c = (a == b) ? 1 : 0;      break;
              case GTE: c = (a >= b) ? 1 : 0;      break;
              case GTN: c = (a >  b) ? 1 : 0;      break;
              case NEQ: c = (a != b) ? 1 : 0;      break;
            }}
            u64 done = U_32(c);
            clear(mem, get_loc(term,0), 2);
            link(mem, host, done);
            break;
          }}
          if (get_tag(arg0) == PAR) {{
            inc_cost(mem);
            u64 op20 = get_loc(term, 0);
            u64 op21 = get_loc(arg0, 0);
            u64 let0 = alloc(mem, 3);
            u64 par0 = alloc(mem, 2);
            link(mem, let0+2, arg1);
            link(mem, op20+1, Dp0(get_ext(arg0), let0));
            link(mem, op20+0, ask_arg(mem, arg0, 0));
            link(mem, op21+0, ask_arg(mem, arg0, 1));
            link(mem, op21+1, Dp1(get_ext(arg0), let0));
            link(mem, par0+0, Op2(get_ext(term), op20));
            link(mem, par0+1, Op2(get_ext(term), op21));
            u64 done = Par(get_ext(arg0), par0);
            link(mem, host, done);
            break;
          }}
          if (get_tag(arg1) == PAR) {{
            inc_cost(mem);
            u64 op20 = get_loc(term, 0);
            u64 op21 = get_loc(arg1, 0);
            u64 let0 = alloc(mem, 3);
            u64 par0 = alloc(mem, 2);
            link(mem, let0+2, arg0);
            link(mem, op20+0, Dp0(get_ext(arg1), let0));
            link(mem, op20+1, ask_arg(mem, arg1, 0));
            link(mem, op21+1, ask_arg(mem, arg1, 1));
            link(mem, op21+0, Dp1(get_ext(arg1), let0));
            link(mem, par0+0, Op2(get_ext(term), op20));
            link(mem, par0+1, Op2(get_ext(term), op21));
            u64 done = Par(get_ext(arg1), par0);
            link(mem, host, done);
            break;
          }}
          break;
        }}
        case FUN: {{
          u64 fun = get_ext(term);
          u64 ari = get_ari(term);

          switch (fun)
          //GENERATED_REWRITE_RULES_STEP_1_START//
          {{
{}
          }}
          //GENERATED_REWRITE_RULES_STEP_1_END//

          break;
        }}
      }}
    }}

    u64 item = stk_pop(&stack);
    if (item == -1) {{
      break;
    }} else {{
      init = item >> 31;
      host = item & 0x7FFFFFFF;
      continue;
    }}

  }}

  return ask_lnk(mem, root);
}}

// sets the nth bit of a bit-array represented as a u64 array
void set_bit(u64* bits, u64 bit) {{
  bits[bit >> 6] |= (1ULL << (bit & 0x3f));
}}

// gets the nth bit of a bit-array represented as a u64 array
u8 get_bit(u64* bits, u64 bit) {{
  return (bits[bit >> 6] >> (bit & 0x3F)) & 1;
}}

#ifdef PARALLEL
void normal_fork(u64 tid, u64 host);
u64  normal_join(u64 tid);
u64 can_spawn = 1;
#endif

Lnk normal(Worker* mem, u64 host) {{
  Lnk term = ask_lnk(mem, host);
  //printf("normal "); debug_print_lnk(term); printf("\n");
  if (get_bit(seen_data, host)) {{
    return term;
  }} else {{
    term = reduce(mem, host, 0);
    set_bit(seen_data, host);
    u64 rec_size = 0;
    u64 rec_locs[16];
    switch (get_tag(term)) {{
      case LAM: {{
        rec_locs[rec_size++] = get_loc(term,1);
        break;
      }}
      case APP: {{
        rec_locs[rec_size++] = get_loc(term,0);
        rec_locs[rec_size++] = get_loc(term,1);
        break;
      }}
      case PAR: {{
        rec_locs[rec_size++] = get_loc(term,0);
        rec_locs[rec_size++] = get_loc(term,1);
        break;
      }}
      case DP0: {{
        rec_locs[rec_size++] = get_loc(term,2);
        break;
      }}
      case DP1: {{
        rec_locs[rec_size++] = get_loc(term,2);
        break;
      }}
      case CTR: case FUN: {{
        u64 arity = (u64)get_ari(term);
        for (u64 i = 0; i < arity; ++i) {{
          rec_locs[rec_size++] = get_loc(term,i);
        }}
        break;
      }}
    }}
    #ifdef PARALLEL
    // TODO: create worker stack, allow re-spawning workers
    if (can_spawn && rec_size > 1 && rec_size <= MAX_WORKERS) {{
      can_spawn = 0;

      for (u64 tid = 1; tid < rec_size; ++tid) {{
        //printf("[%llu] spawn %llu\n", mem->tid, tid);
        normal_fork(tid, rec_locs[tid]);
      }}

      link(mem, rec_locs[0], normal(mem, rec_locs[0]));

      for (u64 tid = 1; tid < rec_size; ++tid) {{
        //printf("[%llu] join %llu\n", mem->tid, tid);
        link(mem, rec_locs[tid], normal_join(tid));
      }}

    }} else {{
      for (u64 i = 0; i < rec_size; ++i) {{
        link(mem, rec_locs[i], normal(mem, rec_locs[i]));
      }}
    }}
    #else
    for (u64 i = 0; i < rec_size; ++i) {{
      link(mem, rec_locs[i], normal(mem, rec_locs[i]));
    }}
    #endif

    return term;
  }}
}}

#ifdef PARALLEL

// Normalizes in a separate thread
void normal_fork(u64 tid, u64 host) {{
  pthread_mutex_lock(&workers[tid].has_work_mutex);
  workers[tid].has_work = host;
  pthread_cond_signal(&workers[tid].has_work_signal);
  pthread_mutex_unlock(&workers[tid].has_work_mutex);
}}

// Waits the result of a forked normalizer
u64 normal_join(u64 tid) {{
  while (1) {{
    pthread_mutex_lock(&workers[tid].has_result_mutex);
    while (workers[tid].has_result == -1) {{
      pthread_cond_wait(&workers[tid].has_result_signal, &workers[tid].has_result_mutex);
    }}
    u64 done = workers[tid].has_result;
    workers[tid].has_result = -1;
    pthread_mutex_unlock(&workers[tid].has_result_mutex);
    return done;
  }}
}}

// The normalizer worker
void *worker(void *arg) {{
  u64 tid = (u64)arg;
  while (1) {{
    pthread_mutex_lock(&workers[tid].has_work_mutex);
    while (workers[tid].has_work == -1) {{
      pthread_cond_wait(&workers[tid].has_work_signal, &workers[tid].has_work_mutex);
    }}
    workers[tid].has_result = normal(&workers[tid], workers[tid].has_work);
    workers[tid].has_work = -1;
    pthread_cond_signal(&workers[tid].has_result_signal);
    pthread_mutex_unlock(&workers[tid].has_work_mutex);
  }}
  return 0;
}}

#endif

u64 ffi_cost;
u64 ffi_size;

void ffi_normal(u8* mem_data, u32 mem_size, u32 host) {{

  // Inits seen
  for (u64 i = 0; i < seen_size; ++i) {{
    seen_data[i] = 0;
  }}

  // Init thread objects
  for (u64 t = 0; t < MAX_WORKERS; ++t) {{
    workers[t].tid = t;
    workers[t].size = t == 0 ? (u64)mem_size : 0l;
    workers[t].node = (u64*)mem_data;
    for (u64 a = 0; a < MAX_ARITY; ++a) {{
      stk_init(&workers[t].free[a]);
    }}
    workers[t].cost = 0;
    #ifdef PARALLEL
    workers[t].has_work = -1;
    pthread_mutex_init(&workers[t].has_work_mutex, NULL);
    pthread_cond_init(&workers[t].has_work_signal, NULL);
    workers[t].has_result = -1;
    pthread_mutex_init(&workers[t].has_result_mutex, NULL);
    pthread_cond_init(&workers[t].has_result_signal, NULL);
    workers[t].thread = NULL;
    #endif
  }}

  // Spawns threads
  #ifdef PARALLEL
  for (u64 tid = 1; tid < MAX_WORKERS; ++tid) {{
    pthread_create(&workers[tid].thread, NULL, &worker, (void*)tid);
  }}
  #endif

  // Normalizes trm
  normal(&workers[0], (u64) host);
  
  // Computes total cost and size
  ffi_cost = 0;
  ffi_size = 0;
  for (u64 tid = 0; tid < MAX_WORKERS; ++tid) {{
    ffi_cost += workers[tid].cost;
    ffi_size += workers[tid].size;
  }}

  // TODO: stop pending threads

  // Clears workers
  for (u64 tid = 0; tid < MAX_WORKERS; ++tid) {{
    for (u64 a = 0; a < MAX_ARITY; ++a) {{
      stk_free(&workers[tid].free[a]);
    }}
    #ifdef PARALLEL
    pthread_mutex_destroy(&workers[tid].has_work_mutex);
    pthread_cond_destroy(&workers[tid].has_work_signal);
    pthread_mutex_destroy(&workers[tid].has_result_mutex);
    pthread_cond_destroy(&workers[tid].has_result_signal);
    #endif
  }}
}}

// Main
// ----

// Uncomment to test without Deno FFI
int main() {{
  Worker mem;
  mem.size = 1;
  mem.node = (u64*)malloc(2 * 268435456 * sizeof(u64)); // 4gb
  mem.node[0] = Cal(0, MAIN, 0);
  printf("Reducing...\n");
  ffi_normal((u8*)mem.node, mem.size, 0);
  printf("Done!\n");
  free(mem.node);
  printf("rwt: %llu\n", ffi_cost);
}}
  "#, c_ids, inits,  codes);
}

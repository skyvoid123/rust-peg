use codegen::RustWriter;
use std::str;

pub struct Grammar {
	initializer: Option<~str>,
	rules: ~[~Rule]
}

pub struct Rule {
	name: ~str,
	expr: ~Expr,
	ret_type: ~str,
	exported: bool,
}

pub struct CharSetCase {
	start: char,
	end: char
}

pub struct TaggedExpr {
	name: Option<~str>,
	expr: ~Expr
}

pub enum Expr {
	AnyCharExpr,
	LiteralExpr(~str),
	CharSetExpr(bool, ~[CharSetCase]),
	RuleExpr(~str),
	SequenceExpr(~[~Expr]),
	ChoiceExpr(~[~Expr]),
	OptionalExpr(~Expr),
	ZeroOrMore(~Expr),
	OneOrMore(~Expr),
	DelimitedExpr(~Expr, ~Expr),
	PosAssertExpr(~Expr),
	NegAssertExpr(~Expr),
	StringifyExpr(~Expr),
	ActionExpr(~[TaggedExpr], ~str),
}

pub fn compile_grammar(w: &RustWriter, grammar: &Grammar) {
	compile_header(w, grammar.initializer.as_ref().map_or("", |s| s.as_slice()));

	for rule in grammar.rules.iter() {
		compile_rule(w, *rule);
	}
}

fn compile_header(w: &RustWriter, header: &str) {
	w.write("// Generated by rust-peg. Do not edit.
extern mod std;
use std::str::{CharRange};
    ");
 
 	w.write(header);

 	w.write("
#[inline]
fn slice_eq(input: &str, pos: uint, m: &str) -> Result<(uint, ()), uint> {
    let l = m.len();
    if (input.len() >= pos + l && input.slice(pos, pos+l) == m) {
        Ok((pos+l, ()))
    } else {
        Err(pos)
    }
}

#[inline]
fn any_char(input: &str, pos: uint) -> Result<(uint, ()), uint> {
    if input.len() > pos {
        Ok((input.char_range_at(pos).next, ()))
    } else {
        Err(pos)
    }
}

fn pos_to_line(input: &str, pos: uint) -> uint {
	let mut remaining = pos as int;
	let mut lineno: uint = 1;
	for line in input.lines() {
		remaining -= (line.len() as int) + 1;
		if (remaining <= 0) {
			return lineno;
		}
		lineno+=1;
	}
	return lineno;
}
");
}


fn compile_rule(w: &RustWriter, rule: &Rule) {
	w.line("#[allow(unused_variable)]");
	w.def_fn(false, "parse_"+rule.name, "input: &str, pos: uint", "Result<(uint, " + rule.ret_type + ") , uint>", || {
		compile_expr(w, rule.expr, rule.ret_type != ~"()");
	});

	if rule.exported {
		compile_rule_export(w, rule);
	}
}

fn compile_rule_export(w: &RustWriter, rule: &Rule) {
	w.def_fn(true, rule.name, "input: &str", "Result<"+rule.ret_type+", ~str>", || {
		w.match_block("parse_"+rule.name+"(input, 0)", || {
			w.match_case("Ok((pos, value))", || {
				w.if_else("pos == input.len()",
					|| { w.line("Ok(value)"); },
					|| { w.line("Err(~\"Expected end of input at \" + pos_to_line(input, pos).to_str())"); }
				)
			});
			w.match_inline_case("Err(pos)", "Err(\"Error at \"+ pos_to_line(input, pos).to_str())");
		});
	});
}

fn compile_match_and_then(w: &RustWriter, e: &Expr, value_name: Option<&str>, then: ||) {
	w.let_block("seq_res", || {
		compile_expr(w, e, value_name.is_some());
	});
	w.match_block("seq_res", || {
		w.match_inline_case("Err(pos)", "Err(pos)");
		w.match_case("Ok((pos, "+value_name.unwrap_or("_")+"))", || {
			then();
		});
	});
}

fn compile_zero_or_more(w: &RustWriter, e: &Expr, list_initial: Option<&str>) {
	w.let_mut_stmt("repeat_pos", "pos");
	let result_used = list_initial.is_some();
	if (result_used) {
		w.let_mut_stmt("repeat_value", list_initial.unwrap());
	}
	w.loop_block(|| {
		w.let_block("step_res", || {
			w.let_stmt("pos", "repeat_pos");
			compile_expr(w, e, result_used);
		});
		w.match_block("step_res", || {
			let match_arm = if result_used {
				"Ok((newpos, value))"
			} else {
				"Ok((newpos, _))"
			};
			w.match_case(match_arm, || {
				w.line("repeat_pos = newpos;");
				if result_used {
					w.line("repeat_value.push(value);");
				}
			});
			w.match_case("Err(..)", || {
				w.line("break;");
			});
		});
	});
	if result_used {
		w.line("Ok((repeat_pos, repeat_value))");
	} else {
		w.line("Ok((repeat_pos, ()))");
	}
}

fn compile_expr(w: &RustWriter, e: &Expr, result_used: bool) {
	match *e {
		AnyCharExpr => { 
			w.line("any_char(input, pos)");
			/*w.if_else("input.len() > pos",
				||{ w.line("Ok(pos+1)"); },
				||{ w.line("Err(pos)"); }
			);*/
		}

		LiteralExpr(ref s) => {
			w.line("slice_eq(input, pos, \""+s.escape_default()+"\")");
			/*w.if_else("slice_eq(input, pos, \""+*s+"\")",
				||{ w.line("Ok(pos+" + s.len().to_str() + ")"); },
				||{ w.line("Err(pos)"); }
			);*/
		}

		CharSetExpr(invert, ref cases) => {
			let result_strs = ("Ok((next, ()))", "Err(pos)");
			let (y_str, n_str) = if !invert { result_strs } else { result_strs.swap() };

			w.if_else("input.len() > pos",
				|| {
					w.line("let CharRange {ch, next} = input.char_range_at(pos);");
					w.match_block("ch", || {
						w.write_indent();
						for (i, case) in cases.iter().enumerate() {
							if i != 0 { w.write(" | "); }
							if case.start == case.end {
								w.write("'"+str::from_char(case.start).escape_default()+"'");
							} else {
								let start = str::from_char(case.start).escape_default();
								let end = str::from_char(case.end).escape_default();
								w.write("'"+start+"'..'"+end+"'");
							}
						}
						w.write(" => { "+y_str+" }\n");
						w.match_inline_case("_", n_str);
					});
				},
				|| { w.line("Err(pos)"); }
			)
		}
		
		RuleExpr(ref ruleName) => {
			w.line("parse_"+*ruleName+"(input, pos)");
		}

		SequenceExpr(ref exprs) => {
			fn write_seq(w: &RustWriter, exprs: &[~Expr]) {
				if (exprs.len() == 1) {
					compile_expr(w, exprs[0], false);
				} else {
					compile_match_and_then(w, exprs[0], None, || {
						write_seq(w, exprs.tail());
					});
				}
			}

			if (exprs.len() > 0 ) {
				write_seq(w, *exprs);
			}
		}

		ChoiceExpr(ref exprs) => {
			fn write_choice(w: &RustWriter, exprs: &[~Expr], result_used: bool) {
				if (exprs.len() == 1) {
					compile_expr(w, exprs[0], result_used);
				} else {
					w.let_block("choice_res", || {
						compile_expr(w, exprs[0], result_used);
					});
					w.match_block("choice_res", || {
						w.match_inline_case("Ok((pos, value))", "Ok((pos, value))");
						w.match_case("Err(..)", || {
							write_choice(w, exprs.tail(), result_used);
						});
					});
				}
			}

			if (exprs.len() > 0 ) {
				write_choice(w, *exprs, result_used);
			}
		}

		OptionalExpr(ref e) => {
			w.let_block("optional_res", || {
				compile_expr(w, *e, result_used);
			});
			w.match_block("optional_res", || {
				w.match_inline_case("Ok((newpos, value))", "Ok((newpos, Some(value)))");
				w.match_inline_case("Err(..)", "Ok((pos, None))");
			});
		}
		
		ZeroOrMore(ref e) => {
			compile_zero_or_more(w, *e, if result_used { Some("~[]") } else { None });
		}

		OneOrMore(ref e) => {
			compile_match_and_then(w, *e, if result_used { Some("first_value") } else { None }, || {
				compile_zero_or_more(w, *e, if result_used { Some("~[first_value]") } else { None });
			});
		}
		
		DelimitedExpr(_, _) => fail!("not implemented"),
		StringifyExpr(..) => fail!("not implemented"),

		PosAssertExpr(ref e) => {
			w.let_block("assert_res", || {
				compile_expr(w, *e, false);
			});
			w.match_block("assert_res", || {
				w.match_inline_case("Ok(..)", "Ok((pos, ()))");
				w.match_inline_case("Err(..)", "Err(pos)");
			});
		}

		NegAssertExpr(ref e) => {
			w.let_block("neg_assert_res", || {
				compile_expr(w, *e, false);
			});
			w.match_block("neg_assert_res", || {
				w.match_inline_case("Err(..)", "Ok((pos, ()))");
				w.match_inline_case("Ok(..)", "Err(pos)");
			});
		}

		ActionExpr(ref exprs, ref code) => {
			w.let_stmt("start_pos", "pos");
			fn write_seq(w: &RustWriter, exprs: &[TaggedExpr], code: &str) {
				if (exprs.len() > 0) {
					let name = exprs.head().name.as_ref().map(|s| s.as_slice());
					compile_match_and_then(w, exprs.head().expr, name, || {
						write_seq(w, exprs.tail(), code);
					});
				} else {
					w.let_stmt("match_str",  "input.slice(start_pos, pos);");
					w.write_indent();
					w.write("Ok((pos, {");
					w.write(code);
					w.write("}))\n");
				}
			}

			write_seq(w, *exprs, *code);
		}
	}
}


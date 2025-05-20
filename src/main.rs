use std::{collections::HashMap, fs, env};
use anyhow::Result;
use walkdir::WalkDir;
use swc_common::{sync::Lrc, SourceMap, FileName};
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_visit::{Visit, VisitWith};
use swc_ecma_ast::{ImportDecl, Ident};

struct Analyzer {
    imports: Vec<String>,
    usage: HashMap<String, usize>,
}

impl Analyzer {
    fn new() -> Self {
        Self {
            imports: Vec::new(),
            usage: HashMap::new(),
        }
    }
}

impl Visit for Analyzer {
    fn visit_import_decl(&mut self, n: &ImportDecl) {
        for spec in &n.specifiers {
            let name = match spec {
                swc_ecma_ast::ImportSpecifier::Named(named) => named.local.sym.to_string(),
                swc_ecma_ast::ImportSpecifier::Default(def) => def.local.sym.to_string(),
                swc_ecma_ast::ImportSpecifier::Namespace(ns) => ns.local.sym.to_string(),
            };
            self.imports.push(name);
        }
        n.visit_children_with(self);
    }

    fn visit_ident(&mut self, ident: &Ident) {
        let key = ident.sym.to_string();
        if self.imports.contains(&key) {
            *self.usage.entry(key).or_insert(0) += 1;
        }
    }
}

fn main() -> Result<()> {
    // 解析対象ディレクトリをコマンドライン引数から取得。未指定ならカレントディレクトリ
    let target = env::args().nth(1).unwrap_or_else(|| ".".into());

    // グローバル集計マップと SourceMap 準備
    let mut global_counts: HashMap<String, usize> = HashMap::new();
    let cm: Lrc<SourceMap> = Default::default();

    // 再帰的に .ts/.tsx ファイルだけを走査 (.d.ts は除外)
    for entry in WalkDir::new(&target)
        .into_iter()
        .filter_entry(|e| {
            let p = e.path().to_string_lossy();
            !p.contains("node_modules")
                && !p.contains(".vscode")
                && !p.contains(".angular")
                && !p.contains(".git")
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path().to_string_lossy();
            if p.ends_with(".d.ts") {
                return false;
            }
            matches!(
                e.path()
                    .extension()
                    .and_then(|s| s.to_str()),
                Some("ts") | Some("tsx")
            )
        })
    {
        let path = entry.path();

        // ソース読み込み＆SourceFile化
        let src = fs::read_to_string(path)?;
        let fm = cm.new_source_file(FileName::Real(path.to_path_buf()).into(), src.clone());

        // 拡張子ごとに TSX モード切替 (tsx のときだけ true)
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let syntax = Syntax::Typescript(TsSyntax {
            tsx: ext == "tsx",
            decorators: true, // Angular の @Component 等を許可
            ..Default::default()
        });

        let mut parser = Parser::new(syntax, StringInput::from(&*fm), None);

        // パース失敗したらスキップして次へ
        let module = match parser.parse_module() {
            Ok(m) => m,
            Err(err) => {
                eprintln!("⚠️ 解析スキップ: {}: {:?}", path.display(), err);
                continue;
            }
        };

        // AST をトラバースして imports と usage を収集
        let mut analyzer = Analyzer::new();
        module.visit_with(&mut analyzer);

        // ファイルごとの結果をグローバル集計へマージ
        for (k, v) in analyzer.usage {
            *global_counts.entry(k).or_insert(0) += v;
        }
    }

    // 最終結果を降順ソートして出力
    let mut sorted: Vec<_> = global_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\n===== インポート名／使用回数（多い順） =====");
    for (name, count) in sorted {
        println!("{:<30} {}", name, count);
    }

    Ok(())
}

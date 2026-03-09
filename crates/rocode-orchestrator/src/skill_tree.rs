use rocode_provider::{Content, Message, Role};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTreeNode {
    pub node_id: String,
    pub markdown_path: String,
    pub children: Vec<SkillTreeNode>,
}

impl SkillTreeNode {
    pub fn new(node_id: impl Into<String>, markdown_path: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            markdown_path: markdown_path.into(),
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<SkillTreeNode>) -> Self {
        self.children = children;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledSkillNode {
    pub node_id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub lineage: Vec<String>,
    pub source_paths: Vec<String>,
    pub context_markdown: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompiledSkillTree {
    pub nodes: Vec<CompiledSkillNode>,
}

impl CompiledSkillTree {
    pub fn node(&self, node_id: &str) -> Option<&CompiledSkillNode> {
        self.nodes.iter().find(|n| n.node_id == node_id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SkillTreeCompileError {
    #[error("skill tree references missing markdown path: {path}")]
    MissingMarkdown { path: String },

    #[error("skill tree has duplicate node_id: {node_id}")]
    DuplicateNodeId { node_id: String },
}

#[derive(Debug, Clone)]
pub struct SkillTreeCompiler {
    context_separator: String,
}

impl Default for SkillTreeCompiler {
    fn default() -> Self {
        Self {
            context_separator: "\n\n---\n\n".to_string(),
        }
    }
}

impl SkillTreeCompiler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.context_separator = separator.into();
        self
    }

    pub fn compile(
        &self,
        root: &SkillTreeNode,
        markdown_repo: &HashMap<String, String>,
    ) -> Result<CompiledSkillTree, SkillTreeCompileError> {
        let mut visited = HashSet::new();
        let mut compiled_nodes = Vec::new();
        let mut lineage = Vec::<String>::new();
        let mut inherited_paths = Vec::<String>::new();
        let mut inherited_segments = Vec::<String>::new();

        self.compile_node(
            root,
            None,
            0,
            markdown_repo,
            &mut visited,
            &mut lineage,
            &mut inherited_paths,
            &mut inherited_segments,
            &mut compiled_nodes,
        )?;

        Ok(CompiledSkillTree {
            nodes: compiled_nodes,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_node(
        &self,
        node: &SkillTreeNode,
        parent_id: Option<&str>,
        depth: usize,
        markdown_repo: &HashMap<String, String>,
        visited: &mut HashSet<String>,
        lineage: &mut Vec<String>,
        inherited_paths: &mut Vec<String>,
        inherited_segments: &mut Vec<String>,
        out: &mut Vec<CompiledSkillNode>,
    ) -> Result<(), SkillTreeCompileError> {
        if !visited.insert(node.node_id.clone()) {
            return Err(SkillTreeCompileError::DuplicateNodeId {
                node_id: node.node_id.clone(),
            });
        }

        let markdown = markdown_repo
            .get(&node.markdown_path)
            .ok_or_else(|| SkillTreeCompileError::MissingMarkdown {
                path: node.markdown_path.clone(),
            })?
            .clone();

        lineage.push(node.node_id.clone());
        inherited_paths.push(node.markdown_path.clone());
        inherited_segments.push(markdown);

        let context_markdown = inherited_segments.join(&self.context_separator);
        out.push(CompiledSkillNode {
            node_id: node.node_id.clone(),
            parent_id: parent_id.map(str::to_string),
            depth,
            lineage: lineage.clone(),
            source_paths: inherited_paths.clone(),
            context_markdown,
        });

        for child in &node.children {
            self.compile_node(
                child,
                Some(&node.node_id),
                depth + 1,
                markdown_repo,
                visited,
                lineage,
                inherited_paths,
                inherited_segments,
                out,
            )?;
        }

        lineage.pop();
        inherited_paths.pop();
        inherited_segments.pop();

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTreeRequestPlan {
    #[serde(alias = "contextMarkdown")]
    pub context_markdown: String,
}

impl SkillTreeRequestPlan {
    const CONTEXT_HEADER: &'static str = "Skill Tree Context (Inherited):";

    pub fn from_tree(
        root: &SkillTreeNode,
        markdown_repo: &HashMap<String, String>,
    ) -> Result<Option<Self>, SkillTreeCompileError> {
        Self::from_tree_with_separator(root, markdown_repo, None)
    }

    pub fn from_tree_with_separator(
        root: &SkillTreeNode,
        markdown_repo: &HashMap<String, String>,
        separator: Option<&str>,
    ) -> Result<Option<Self>, SkillTreeCompileError> {
        let compiler = match separator {
            Some(separator) => SkillTreeCompiler::new().with_separator(separator.to_string()),
            None => SkillTreeCompiler::new(),
        };
        let compiled = compiler.compile(root, markdown_repo)?;
        Ok(Self::from_compiled(compiled))
    }

    pub fn from_compiled(compiled: CompiledSkillTree) -> Option<Self> {
        let root = compiled
            .nodes
            .iter()
            .find(|node| node.depth == 0)
            .or_else(|| compiled.nodes.first())?;
        let context_markdown = root.context_markdown.trim().to_string();
        if context_markdown.is_empty() {
            None
        } else {
            Some(Self { context_markdown })
        }
    }

    pub fn compose_system_prompt(&self, base: Option<&str>) -> Option<String> {
        let context = self.context_markdown.trim();
        let base = base.unwrap_or("").trim();

        match (base.is_empty(), context.is_empty()) {
            (true, true) => None,
            (false, true) => Some(base.to_string()),
            (true, false) => Some(format!("{}\n{}", Self::CONTEXT_HEADER, context)),
            (false, false) => Some(format!("{}\n\n{}\n{}", base, Self::CONTEXT_HEADER, context)),
        }
    }

    pub fn apply_to_messages(&self, mut messages: Vec<Message>) -> Vec<Message> {
        let existing_system =
            messages
                .first()
                .and_then(|message| match (&message.role, &message.content) {
                    (Role::System, Content::Text(text)) => Some(text.as_str()),
                    _ => None,
                });

        let Some(system_prompt) = self.compose_system_prompt(existing_system) else {
            return messages;
        };

        if let Some(first) = messages.first_mut() {
            if matches!(first.role, Role::System) {
                first.content = Content::Text(system_prompt);
                return messages;
            }
        }

        messages.insert(0, Message::system(system_prompt));
        messages
    }
}

pub fn resolve_skill_markdown_repo(
    skill_paths: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut repo = HashMap::with_capacity(skill_paths.len());
    for (key, value) in skill_paths {
        let trimmed = value.trim();
        let looks_inline =
            trimmed.contains('\n') || trimmed.starts_with('#') || trimmed.starts_with("```");
        if looks_inline {
            repo.insert(key.clone(), value.clone());
            continue;
        }

        let raw_path = trimmed.strip_prefix("file://").unwrap_or(trimmed);
        match fs::read_to_string(raw_path) {
            Ok(content) => {
                repo.insert(key.clone(), content);
            }
            Err(_) => {
                repo.insert(key.clone(), value.clone());
            }
        }
    }
    repo
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn repo(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn request_plan_composes_system_prompt() {
        let plan = SkillTreeRequestPlan {
            context_markdown: "ROOT".to_string(),
        };

        assert_eq!(
            plan.compose_system_prompt(Some("BASE")).as_deref(),
            Some("BASE\n\nSkill Tree Context (Inherited):\nROOT")
        );
        assert_eq!(
            plan.compose_system_prompt(None).as_deref(),
            Some("Skill Tree Context (Inherited):\nROOT")
        );
    }

    #[test]
    fn request_plan_applies_to_messages() {
        let plan = SkillTreeRequestPlan {
            context_markdown: "ROOT".to_string(),
        };

        let messages = plan.apply_to_messages(vec![Message::user("hello")]);
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, Role::System));
    }

    #[test]
    fn compile_single_node_tree() {
        let root = SkillTreeNode::new("root", "docs/root.md");
        let compiler = SkillTreeCompiler::new();
        let tree = compiler
            .compile(&root, &repo(&[("docs/root.md", "# Root Rule")]))
            .unwrap();

        assert_eq!(tree.nodes.len(), 1);
        let compiled = tree.node("root").unwrap();
        assert_eq!(compiled.depth, 0);
        assert_eq!(compiled.parent_id, None);
        assert_eq!(compiled.lineage, vec!["root".to_string()]);
        assert_eq!(compiled.source_paths, vec!["docs/root.md".to_string()]);
        assert_eq!(compiled.context_markdown, "# Root Rule");
    }

    #[test]
    fn compile_inherits_context_depth_first() {
        let root = SkillTreeNode::new("root", "docs/root.md")
            .with_children(vec![SkillTreeNode::new("child", "docs/child.md")
                .with_children(vec![SkillTreeNode::new("leaf", "docs/leaf.md")])]);

        let compiler = SkillTreeCompiler::new();
        let tree = compiler
            .compile(
                &root,
                &repo(&[
                    ("docs/root.md", "ROOT"),
                    ("docs/child.md", "CHILD"),
                    ("docs/leaf.md", "LEAF"),
                ]),
            )
            .unwrap();

        let leaf = tree.node("leaf").unwrap();
        assert_eq!(leaf.depth, 2);
        assert_eq!(
            leaf.lineage,
            vec!["root".to_string(), "child".to_string(), "leaf".to_string()]
        );
        assert_eq!(
            leaf.source_paths,
            vec![
                "docs/root.md".to_string(),
                "docs/child.md".to_string(),
                "docs/leaf.md".to_string()
            ]
        );
        assert_eq!(leaf.context_markdown, "ROOT\n\n---\n\nCHILD\n\n---\n\nLEAF");
    }

    #[test]
    fn compile_sibling_context_is_isolated() {
        let root = SkillTreeNode::new("root", "docs/root.md").with_children(vec![
            SkillTreeNode::new("a", "docs/a.md"),
            SkillTreeNode::new("b", "docs/b.md"),
        ]);
        let compiler = SkillTreeCompiler::new();
        let tree = compiler
            .compile(
                &root,
                &repo(&[
                    ("docs/root.md", "ROOT"),
                    ("docs/a.md", "A"),
                    ("docs/b.md", "B"),
                ]),
            )
            .unwrap();

        let a = tree.node("a").unwrap();
        let b = tree.node("b").unwrap();
        assert_eq!(a.context_markdown, "ROOT\n\n---\n\nA");
        assert_eq!(b.context_markdown, "ROOT\n\n---\n\nB");
    }

    #[test]
    fn compile_rejects_duplicate_node_id() {
        let root = SkillTreeNode::new("root", "docs/root.md").with_children(vec![
            SkillTreeNode::new("dup", "docs/a.md"),
            SkillTreeNode::new("dup", "docs/b.md"),
        ]);
        let compiler = SkillTreeCompiler::new();
        let err = compiler
            .compile(
                &root,
                &repo(&[
                    ("docs/root.md", "ROOT"),
                    ("docs/a.md", "A"),
                    ("docs/b.md", "B"),
                ]),
            )
            .unwrap_err();

        match err {
            SkillTreeCompileError::DuplicateNodeId { node_id } => assert_eq!(node_id, "dup"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn compile_rejects_missing_markdown_path() {
        let root = SkillTreeNode::new("root", "docs/root.md")
            .with_children(vec![SkillTreeNode::new("child", "docs/missing.md")]);
        let compiler = SkillTreeCompiler::new();
        let err = compiler
            .compile(&root, &repo(&[("docs/root.md", "ROOT")]))
            .unwrap_err();

        match err {
            SkillTreeCompileError::MissingMarkdown { path } => {
                assert_eq!(path, "docs/missing.md")
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}

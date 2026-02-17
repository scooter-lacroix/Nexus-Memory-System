//! Graph tree structure for hierarchical memory organization
//!
//! This module implements a graph tree that organizes memories hierarchically
//! for efficient resource management and improved semantic search.
//!
//! ## Tree Structure
//! - Root nodes: Category containers
//! - Lane type nodes: Optional intermediate organization
//! - Leaf nodes: Actual memory items
//!
//! ## Relevance Boosting
//! - Priority weights: High (1.5), Medium (1.2), Low (1.0)
//! - Depth penalty: Slight reduction for deeper nodes
//! - Ancestor boost: Aggregated parent weights

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Unique identifier for tree nodes
pub type NodeId = i64;

/// Graph tree node representing a memory or category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Unique node identifier (memory ID or category node ID)
    pub id: NodeId,

    /// Node type
    pub node_type: NodeType,

    /// Parent node ID (None for root)
    pub parent_id: Option<NodeId>,

    /// Child node IDs
    pub children: Vec<NodeId>,

    /// Depth in tree (0 for root)
    pub depth: u32,

    /// Node weight for relevance boosting
    pub weight: f32,

    /// Category this node belongs to
    pub category: String,

    /// Optional memory lane type
    pub memory_lane_type: Option<String>,
}

impl GraphNode {
    /// Create a new graph node
    pub fn new(id: NodeId, node_type: NodeType, category: String) -> Self {
        Self {
            id,
            node_type,
            parent_id: None,
            children: Vec::new(),
            depth: 0,
            weight: 1.0,
            category,
            memory_lane_type: None,
        }
    }

    /// Check if this is a leaf node (has no children)
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Check if this is a root node (has no parent)
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    /// Add a child node
    pub fn add_child(&mut self, child_id: NodeId) {
        if !self.children.contains(&child_id) {
            self.children.push(child_id);
        }
    }

    /// Remove a child node
    pub fn remove_child(&mut self, child_id: NodeId) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child_id) {
            self.children.remove(pos);
            true
        } else {
            false
        }
    }

    /// Set weight based on priority level
    pub fn set_priority_weight(&mut self, priority: u8) {
        self.weight = match priority {
            1 => 1.5,  // High priority
            2 => 1.2,  // Medium priority
            _ => 1.0,  // Low/default priority
        };
    }
}

/// Type of graph node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    /// Category root node
    CategoryRoot,

    /// Memory lane type node
    LaneTypeNode,

    /// Actual memory leaf node
    MemoryLeaf,

    /// Time-based cluster node
    TimeCluster,
}

/// Tree node for traversal operations
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// The graph node
    pub node: GraphNode,

    /// Cumulative path weight from root
    pub path_weight: f32,

    /// Distance from root
    pub distance: u32,
}

impl TreeNode {
    /// Create a new tree node
    pub fn new(node: GraphNode) -> Self {
        let weight = node.weight;
        Self {
            node,
            path_weight: weight,
            distance: 0,
        }
    }

    /// Create a child tree node
    pub fn child(node: GraphNode, parent: &TreeNode) -> Self {
        Self {
            path_weight: parent.path_weight * node.weight,
            distance: parent.distance + 1,
            node,
        }
    }
}

/// Graph tree structure for hierarchical memory organization
#[derive(Debug, Clone, Default)]
pub struct GraphTree {
    /// All nodes indexed by ID
    nodes: HashMap<NodeId, GraphNode>,

    /// Root node IDs
    roots: Vec<NodeId>,

    /// Category to root node mapping
    category_roots: HashMap<String, NodeId>,

    /// Next synthetic node ID (for non-memory nodes)
    next_synthetic_id: NodeId,
}

impl GraphTree {
    /// Create a new empty graph tree
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            roots: Vec::new(),
            category_roots: HashMap::new(),
            next_synthetic_id: -1, // Synthetic IDs are negative
        }
    }

    /// Add a memory node to the tree
    pub fn add_memory(
        &mut self,
        memory_id: NodeId,
        category: &str,
        memory_lane_type: Option<&str>,
        priority: Option<u8>,
    ) {
        // Ensure category root exists
        let category_root_id = self.get_or_create_category_root(category);

        // Create memory node
        let mut node = GraphNode::new(memory_id, NodeType::MemoryLeaf, category.to_string());
        node.parent_id = Some(category_root_id);
        node.memory_lane_type = memory_lane_type.map(|s| s.to_string());

        if let Some(p) = priority {
            node.set_priority_weight(p);
        }

        // Add to category root's children
        if let Some(root) = self.nodes.get_mut(&category_root_id) {
            root.add_child(memory_id);
            node.depth = root.depth + 1;
        }

        self.nodes.insert(memory_id, node);
    }

    /// Remove a memory node from the tree
    pub fn remove_memory(&mut self, memory_id: NodeId) -> bool {
        if let Some(node) = self.nodes.remove(&memory_id) {
            // Remove from parent's children
            if let Some(parent_id) = node.parent_id {
                if let Some(parent) = self.nodes.get_mut(&parent_id) {
                    parent.remove_child(memory_id);
                }
            }
            true
        } else {
            false
        }
    }

    /// Get a node by ID
    pub fn get(&self, id: NodeId) -> Option<&GraphNode> {
        self.nodes.get(&id)
    }

    /// Get all memory IDs in a category
    pub fn get_memories_by_category(&self, category: &str) -> Vec<NodeId> {
        let mut result = Vec::new();

        if let Some(&root_id) = self.category_roots.get(category) {
            self.collect_leaf_ids(root_id, &mut result);
        }

        result
    }

    /// Get all memory IDs with a specific lane type
    pub fn get_memories_by_lane_type(&self, lane_type: &str) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| {
                n.node_type == NodeType::MemoryLeaf
                    && n.memory_lane_type.as_deref() == Some(lane_type)
            })
            .map(|n| n.id)
            .collect()
    }

    /// Get ancestors of a node (path to root)
    pub fn get_ancestors(&self, node_id: NodeId) -> Vec<NodeId> {
        let mut ancestors = Vec::new();
        let mut current = self.nodes.get(&node_id);

        while let Some(node) = current {
            if let Some(parent_id) = node.parent_id {
                ancestors.push(parent_id);
                current = self.nodes.get(&parent_id);
            } else {
                break;
            }
        }

        ancestors
    }

    /// Get descendants of a node (BFS)
    pub fn get_descendants(&self, node_id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut queue = vec![node_id];
        let mut visited = HashSet::new();

        while let Some(id) = queue.pop() {
            if visited.contains(&id) {
                continue;
            }
            visited.insert(id);

            if let Some(node) = self.nodes.get(&id) {
                for &child_id in &node.children {
                    if !visited.contains(&child_id) {
                        result.push(child_id);
                        queue.push(child_id);
                    }
                }
            }
        }

        result
    }

    /// Calculate boosted relevance score based on tree structure
    pub fn calculate_boosted_score(
        &self,
        memory_id: NodeId,
        base_similarity: f32,
    ) -> f32 {
        if let Some(node) = self.nodes.get(&memory_id) {
            // Apply weight from node
            let weight = node.weight;

            // Apply depth penalty (deeper nodes get slightly lower scores)
            let depth_factor = 1.0 - (node.depth as f32 * 0.02);

            // Apply ancestor weight aggregation
            let ancestor_boost = self.calculate_ancestor_boost(memory_id);

            base_similarity * weight * depth_factor.max(0.8) * ancestor_boost
        } else {
            base_similarity
        }
    }

    /// Get tree statistics
    pub fn stats(&self) -> TreeStats {
        let mut stats = TreeStats::default();
        stats.total_nodes = self.nodes.len();
        stats.root_count = self.roots.len();
        stats.category_count = self.category_roots.len();

        for node in self.nodes.values() {
            if node.node_type == NodeType::MemoryLeaf {
                stats.memory_count += 1;
            }
            stats.max_depth = stats.max_depth.max(node.depth);
        }

        stats
    }

    // Private methods

    fn get_or_create_category_root(&mut self, category: &str) -> NodeId {
        if let Some(&id) = self.category_roots.get(category) {
            return id;
        }

        let root_id = self.next_synthetic_id;
        self.next_synthetic_id -= 1;

        let mut root = GraphNode::new(root_id, NodeType::CategoryRoot, category.to_string());
        root.depth = 0;

        self.nodes.insert(root_id, root.clone());
        self.roots.push(root_id);
        self.category_roots.insert(category.to_string(), root_id);

        root_id
    }

    fn collect_leaf_ids(&self, node_id: NodeId, result: &mut Vec<NodeId>) {
        if let Some(node) = self.nodes.get(&node_id) {
            if node.node_type == NodeType::MemoryLeaf {
                result.push(node_id);
            }
            for &child_id in &node.children {
                self.collect_leaf_ids(child_id, result);
            }
        }
    }

    fn calculate_ancestor_boost(&self, node_id: NodeId) -> f32 {
        let ancestors = self.get_ancestors(node_id);
        if ancestors.is_empty() {
            return 1.0;
        }

        let total_weight: f32 = ancestors
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .map(|n| n.weight)
            .product();

        // Normalize to reasonable range
        (total_weight / ancestors.len() as f32).clamp(0.8, 1.2)
    }

    // === Advanced Tree Traversal Algorithms ===

    /// Breadth-first traversal from a starting node
    pub fn traverse_bfs(&self, start_id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        if self.nodes.contains_key(&start_id) {
            queue.push_back(start_id);
            visited.insert(start_id);
        }

        while let Some(node_id) = queue.pop_front() {
            result.push(node_id);

            if let Some(node) = self.nodes.get(&node_id) {
                for &child_id in &node.children {
                    if !visited.contains(&child_id) {
                        visited.insert(child_id);
                        queue.push_back(child_id);
                    }
                }
            }
        }

        result
    }

    /// Depth-first traversal (pre-order) from a starting node
    pub fn traverse_dfs_preorder(&self, start_id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        self.dfs_preorder_helper(start_id, &mut visited, &mut result);
        result
    }

    fn dfs_preorder_helper(
        &self,
        node_id: NodeId,
        visited: &mut HashSet<NodeId>,
        result: &mut Vec<NodeId>,
    ) {
        if visited.contains(&node_id) || !self.nodes.contains_key(&node_id) {
            return;
        }

        visited.insert(node_id);
        result.push(node_id);

        if let Some(node) = self.nodes.get(&node_id) {
            for &child_id in &node.children {
                self.dfs_preorder_helper(child_id, visited, result);
            }
        }
    }

    /// Depth-first traversal (post-order) from a starting node
    pub fn traverse_dfs_postorder(&self, start_id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        self.dfs_postorder_helper(start_id, &mut visited, &mut result);
        result
    }

    fn dfs_postorder_helper(
        &self,
        node_id: NodeId,
        visited: &mut HashSet<NodeId>,
        result: &mut Vec<NodeId>,
    ) {
        if visited.contains(&node_id) || !self.nodes.contains_key(&node_id) {
            return;
        }

        visited.insert(node_id);

        if let Some(node) = self.nodes.get(&node_id) {
            for &child_id in &node.children {
                self.dfs_postorder_helper(child_id, visited, result);
            }
        }

        result.push(node_id);
    }

    /// Get nodes at a specific depth level
    pub fn get_nodes_at_depth(&self, depth: u32) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| n.depth == depth)
            .map(|n| n.id)
            .collect()
    }

    /// Get all leaf nodes (memory entries)
    pub fn get_all_leaves(&self) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| n.is_leaf() && n.node_type == NodeType::MemoryLeaf)
            .map(|n| n.id)
            .collect()
    }

    /// Get path from root to a specific node
    pub fn get_path(&self, node_id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = node_id;

        loop {
            if let Some(node) = self.nodes.get(&current) {
                path.push(current);
                if let Some(parent_id) = node.parent_id {
                    current = parent_id;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        path.reverse(); // Root to leaf order
        path
    }

    /// Find lowest common ancestor of two nodes
    pub fn find_lca(&self, node_a: NodeId, node_b: NodeId) -> Option<NodeId> {
        let mut path_a: HashSet<NodeId> = self.get_ancestors(node_a).into_iter().collect();
        path_a.insert(node_a);

        // Check node_b and its ancestors
        if path_a.contains(&node_b) {
            return Some(node_b);
        }

        let mut current = node_b;
        loop {
            if path_a.contains(&current) {
                return Some(current);
            }

            if let Some(node) = self.nodes.get(&current) {
                if let Some(parent_id) = node.parent_id {
                    current = parent_id;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        None
    }

    /// Calculate the distance between two nodes
    pub fn distance(&self, node_a: NodeId, node_b: NodeId) -> Option<u32> {
        let lca = self.find_lca(node_a, node_b)?;

        let dist_to_lca = |node_id: NodeId| -> u32 {
            let mut dist = 0;
            let mut current = node_id;

            while current != lca {
                if let Some(node) = self.nodes.get(&current) {
                    if let Some(parent_id) = node.parent_id {
                        current = parent_id;
                        dist += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            dist
        };

        Some(dist_to_lca(node_a) + dist_to_lca(node_b))
    }

    /// Get subtree size for a node (including itself)
    pub fn subtree_size(&self, node_id: NodeId) -> usize {
        let descendants = self.get_descendants(node_id);
        descendants.len() + 1 // +1 for the node itself
    }

    /// Prune nodes below a certain depth
    pub fn prune_below_depth(&mut self, max_depth: u32) -> Vec<NodeId> {
        let to_remove: Vec<NodeId> = self
            .nodes
            .values()
            .filter(|n| n.depth > max_depth)
            .map(|n| n.id)
            .collect();

        let mut removed = Vec::new();
        for id in to_remove {
            if self.remove_memory(id) {
                removed.push(id);
            }
        }

        removed
    }

    /// Find all nodes matching a predicate
    pub fn find_matching<F>(&self, predicate: F) -> Vec<NodeId>
    where
        F: Fn(&GraphNode) -> bool,
    {
        self.nodes
            .values()
            .filter(|n| predicate(n))
            .map(|n| n.id)
            .collect()
    }

    /// Get siblings of a node
    pub fn get_siblings(&self, node_id: NodeId) -> Vec<NodeId> {
        let node = match self.nodes.get(&node_id) {
            Some(n) => n,
            None => return Vec::new(),
        };

        let parent_id = match node.parent_id {
            Some(id) => id,
            None => return Vec::new(),
        };

        let parent = match self.nodes.get(&parent_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        parent
            .children
            .iter()
            .filter(|&&id| id != node_id)
            .copied()
            .collect()
    }

    /// Rebalance weights in the tree
    pub fn rebalance_weights(&mut self) {
        // Calculate average weight per level and normalize
        let mut level_weights: HashMap<u32, Vec<f32>> = HashMap::new();

        for node in self.nodes.values() {
            level_weights.entry(node.depth).or_default().push(node.weight);
        }

        let mut level_avgs: HashMap<u32, f32> = HashMap::new();
        for (depth, weights) in level_weights {
            let avg = weights.iter().sum::<f32>() / weights.len() as f32;
            level_avgs.insert(depth, avg);
        }

        // Normalize weights around average
        for node in self.nodes.values_mut() {
            if let Some(&avg) = level_avgs.get(&node.depth) {
                if avg > 0.0 {
                    node.weight = (node.weight / avg).clamp(0.5, 2.0);
                }
            }
        }
    }
}

/// Statistics about the graph tree
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeStats {
    /// Total number of nodes
    pub total_nodes: usize,

    /// Number of root nodes
    pub root_count: usize,

    /// Number of categories
    pub category_count: usize,

    /// Number of memory leaf nodes
    pub memory_count: usize,

    /// Maximum tree depth
    pub max_depth: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_node_new() {
        let node = GraphNode::new(1, NodeType::MemoryLeaf, "general".to_string());
        assert_eq!(node.id, 1);
        assert!(node.is_leaf());
        assert!(node.is_root());
        assert_eq!(node.weight, 1.0);
    }

    #[test]
    fn test_graph_node_add_remove_child() {
        let mut parent = GraphNode::new(1, NodeType::CategoryRoot, "general".to_string());
        parent.add_child(2);
        assert_eq!(parent.children.len(), 1);
        assert!(!parent.is_leaf());

        parent.add_child(2); // Duplicate should not be added
        assert_eq!(parent.children.len(), 1);

        assert!(parent.remove_child(2));
        assert!(parent.is_leaf());
        assert!(!parent.remove_child(999)); // Non-existent
    }

    #[test]
    fn test_graph_node_priority_weight() {
        let mut node = GraphNode::new(1, NodeType::MemoryLeaf, "general".to_string());

        node.set_priority_weight(1);
        assert!((node.weight - 1.5).abs() < 0.01);

        node.set_priority_weight(2);
        assert!((node.weight - 1.2).abs() < 0.01);

        node.set_priority_weight(3);
        assert!((node.weight - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_tree_node_creation() {
        let node = GraphNode::new(1, NodeType::MemoryLeaf, "general".to_string());
        let tree_node = TreeNode::new(node.clone());

        assert_eq!(tree_node.path_weight, 1.0);
        assert_eq!(tree_node.distance, 0);
    }

    #[test]
    fn test_graph_tree_add_memory() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);

        assert!(tree.get(100).is_some());
        let node = tree.get(100).unwrap();
        assert_eq!(node.node_type, NodeType::MemoryLeaf);
        assert!(node.parent_id.is_some());
    }

    #[test]
    fn test_graph_tree_remove_memory() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);

        assert!(tree.remove_memory(100));
        assert!(tree.get(100).is_none());
        assert!(!tree.remove_memory(100)); // Already removed
    }

    #[test]
    fn test_graph_tree_get_by_category() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);
        tree.add_memory(102, "facts", None, None);

        let general = tree.get_memories_by_category("general");
        assert_eq!(general.len(), 2);
        assert!(general.contains(&100));
        assert!(general.contains(&101));

        let facts = tree.get_memories_by_category("facts");
        assert_eq!(facts.len(), 1);
        assert!(facts.contains(&102));
    }

    #[test]
    fn test_graph_tree_get_by_lane_type() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", Some("correction"), None);
        tree.add_memory(101, "general", Some("insight"), None);
        tree.add_memory(102, "facts", Some("correction"), None);

        let corrections = tree.get_memories_by_lane_type("correction");
        assert_eq!(corrections.len(), 2);
    }

    #[test]
    fn test_graph_tree_ancestors() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);

        let ancestors = tree.get_ancestors(100);
        assert_eq!(ancestors.len(), 1); // Category root
    }

    #[test]
    fn test_graph_tree_boosted_score() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", Some("correction"), Some(1)); // High priority

        let score = tree.calculate_boosted_score(100, 0.8);
        // Score should be boosted by priority weight (1.5)
        assert!(score > 0.8);
    }

    #[test]
    fn test_graph_tree_stats() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "facts", None, None);

        let stats = tree.stats();
        assert_eq!(stats.memory_count, 2);
        assert_eq!(stats.category_count, 2);
        assert!(stats.total_nodes >= 4); // 2 memories + 2 category roots
    }

    #[test]
    fn test_traverse_bfs() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);

        // BFS from root should visit category root first, then leaves
        let root_id = tree.category_roots.get("general").copied().unwrap();
        let bfs_order = tree.traverse_bfs(root_id);

        assert!(!bfs_order.is_empty());
        assert_eq!(bfs_order[0], root_id); // Root first
    }

    #[test]
    fn test_traverse_dfs_preorder() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);

        let root_id = tree.category_roots.get("general").copied().unwrap();
        let dfs_order = tree.traverse_dfs_preorder(root_id);

        assert!(!dfs_order.is_empty());
        assert_eq!(dfs_order[0], root_id);
    }

    #[test]
    fn test_get_path() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);

        let path = tree.get_path(100);
        assert_eq!(path.len(), 2); // Category root -> memory
        assert_eq!(path[path.len() - 1], 100);
    }

    #[test]
    fn test_find_lca() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);

        let lca = tree.find_lca(100, 101);
        assert!(lca.is_some());

        // LCA should be the category root
        let root_id = tree.category_roots.get("general").copied();
        assert_eq!(lca, root_id);
    }

    #[test]
    fn test_distance() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);

        let dist = tree.distance(100, 101);
        // Both are children of same parent, distance = 2
        assert_eq!(dist, Some(2));
    }

    #[test]
    fn test_subtree_size() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);

        let root_id = tree.category_roots.get("general").copied().unwrap();
        let size = tree.subtree_size(root_id);

        assert_eq!(size, 3); // Root + 2 memories
    }

    #[test]
    fn test_get_siblings() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);
        tree.add_memory(102, "general", None, None);

        let siblings = tree.get_siblings(100);
        assert_eq!(siblings.len(), 2);
        assert!(siblings.contains(&101));
        assert!(siblings.contains(&102));
    }

    #[test]
    fn test_get_all_leaves() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "facts", None, None);

        let leaves = tree.get_all_leaves();
        assert_eq!(leaves.len(), 2);
    }

    #[test]
    fn test_get_nodes_at_depth() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", None, None);
        tree.add_memory(101, "general", None, None);

        // Memories should be at depth 1
        let depth_1 = tree.get_nodes_at_depth(1);
        assert_eq!(depth_1.len(), 2);
    }

    #[test]
    fn test_find_matching() {
        let mut tree = GraphTree::new();
        tree.add_memory(100, "general", Some("correction"), Some(1));
        tree.add_memory(101, "general", None, None);

        let high_priority = tree.find_matching(|n| n.weight > 1.0);
        assert_eq!(high_priority.len(), 1);
        assert!(high_priority.contains(&100));
    }
}

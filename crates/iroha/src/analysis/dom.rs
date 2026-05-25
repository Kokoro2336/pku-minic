//! Dominance Analysis based on Lengauer-Tarjan algorithm.
//! Reference: https://dl.acm.org/doi/10.1145/357062.357071

#[cfg(feature = "debug")]
use yachiyo::debug::info;

use yachiyo::analysis::Analysis;
pub use yachiyo::analysis::{DomFrontier, DomTree};
use yachiyo::utils::BitSet;

struct BuildDomTree<'a> {
  graph: &'a [(Vec<usize>, Vec<usize>)],
  /// Vertex number -> DFS number
  dfn: Vec<usize>,
  dfn_cnt: usize,
  /// DFS number -> Vertex number
  rev: Vec<usize>,
  /// Vertex number -> Semi-dominator DFS number
  sdom: Vec<usize>,
  /// Vertex number -> vertices that this vertex semi-dominates
  bucket: Vec<Vec<usize>>,
  /// Parent in DSU Forest
  parent: Vec<usize>,
  /// Parent in the DFS Tree
  father: Vec<usize>,
  /// Recording the vertex with the minimum semi-dominator on path sdom[u] -> u
  min: Vec<usize>,
  /// Immediate dominator
  idom: Vec<usize>,

  /// temp structure
  /// Vertex number -> whether visited in DFS
  visited: BitSet,
}

impl<'a> BuildDomTree<'a> {
  pub fn new(graph: &'a [(Vec<usize>, Vec<usize>)]) -> Self {
    Self {
      graph,
      dfn: vec![],
      dfn_cnt: 0,
      rev: vec![],
      sdom: vec![],
      bucket: vec![],
      parent: vec![],
      father: vec![],
      min: vec![],
      idom: vec![],
      visited: BitSet::new(),
    }
  }

  fn init(&mut self) {
    let n = self.graph.len();
    self.dfn = vec![0; n];
    self.dfn_cnt = 0;

    self.rev = vec![0; n];

    self.bucket = vec![vec![]; n];
    self.father = vec![0; n];

    self.parent = (0..n).collect();
    self.sdom = (0..n).collect();
    self.idom = (0..n).collect();
    self.min = (0..n).collect();

    self.visited = BitSet::new();
  }

  fn dfs(&mut self, src: usize) {
    self.visited.insert(src);
    let dfs_num = self.dfn_cnt;
    self.dfn[src] = dfs_num;
    self.rev[dfs_num] = src;
    self.dfn_cnt += 1;

    let succs = self.graph[src].1.clone();
    succs.into_iter().for_each(|succ| {
      if !self.visited.contains(succ) {
        self.father[succ] = src;
        self.dfs(succ);
      }
    })
  }

  fn find(&mut self, u: usize) -> usize {
    if self.parent[u] == u {
      return u;
    }
    let v = self.find(self.parent[u]);
    if self.dfn[self.sdom[self.min[u]]] > self.dfn[self.sdom[self.min[self.parent[u]]]] {
      self.min[u] = self.min[self.parent[u]];
    }
    self.parent[u] = v;
    self.parent[u]
  }

  fn query(&mut self, u: usize) -> usize {
    self.find(u);
    self.min[u]
  }

  fn dfn_min(&mut self, u: usize, v: usize) -> usize {
    if self.dfn[u] < self.dfn[v] {
      u
    } else {
      v
    }
  }

  pub fn build(&mut self) -> DomTree {
    self.init();
    if self.graph.is_empty() {
      return self.export();
    }

    #[cfg(feature = "debug")]
    info!("Start DFS traversal.");

    self.dfs(0);

    #[cfg(feature = "debug")]
    info!("DFS traversal completed. Start computing dominators.");

    let num_visited = self.dfn_cnt;
    for i in (1..num_visited).rev() {
      let u = self.rev[i];

      // find sdom[u]
      let preds = self.graph[u].0.clone();
      for pred in preds {
        if !self.visited.contains(pred) {
          continue;
        }

        if self.dfn[pred] < self.dfn[u] {
          self.sdom[u] = self.dfn_min(self.sdom[u], pred);
        } else {
          let v = self.query(pred);
          self.sdom[u] = self.dfn_min(self.sdom[u], self.sdom[v]);
        }
      }

      // push u to bucket of sdom[u]
      self.bucket[self.sdom[u]].push(u);

      // hang u to father[u] in DSU Forest
      self.parent[u] = self.father[u];

      // evaluate idom of vertices in bucket of father[u]
      // Emm... I have to use a clone due to the bothering borrow checker.
      let father = self.father[u];
      let bucket_len = self.bucket[father].len();
      for i in 0..bucket_len {
        let v = self.bucket[father][i];
        self.idom[v] = self.query(v);
      }
      self.bucket[father].clear();
    }

    // Refine idom
    #[cfg(feature = "debug")]
    info!("Dominator tree computed. Start refining immediate dominators.");

    for i in 0..num_visited {
      let v = self.rev[i];
      let u = self.idom[v];
      // If sdom[u] != sdom[v], then there's a vertex with lower dfn that dominates v, which is idom[u],
      // so we set idom[v] to idom[u].
      // Otherwise, sdom[u] is the immediate dominator of v.
      if self.sdom[u] != self.sdom[v] {
        self.idom[v] = self.idom[u];
      } else {
        self.idom[v] = self.sdom[u];
      }
    }

    // export dom tree
    self.export()
  }

  // FuncId -> DomTree
  pub fn export(&mut self) -> DomTree {
    let mut dom_tree = DomTree::with_len(self.idom.len());
    for idx in 0..self.idom.len() {
      let idom = self.idom[idx];
      if idom != idx {
        dom_tree[idom].push(idx);
      }
    }
    dom_tree
  }
}

struct BuildDomFrontier<'a> {
  graph: &'a [(Vec<usize>, Vec<usize>)],
  dom_tree: &'a DomTree,
  /// Vertex number -> its dominance frontier
  frontier: DomFrontier,
}

impl<'a> BuildDomFrontier<'a> {
  pub fn new(graph: &'a [(Vec<usize>, Vec<usize>)], dom_tree: &'a DomTree) -> Self {
    Self {
      graph,
      dom_tree,
      frontier: DomFrontier::default(),
    }
  }

  pub fn is_dom(&self, dominator: usize, dominatee: usize) -> bool {
    let dom_num = {
      let dom_tree = &self.dom_tree;
      dom_tree[dominator].len()
    };
    if self.dom_tree[dominator].contains(&dominatee) {
      true
    } else {
      // If not direct child, check recursively
      (0..dom_num).any(|child| {
        let child = {
          let dom_tree = &self.dom_tree;
          dom_tree[dominator][child]
        };
        self.is_dom(child, dominatee)
      })
    }
  }

  pub fn compute(&mut self, bb_id: usize) {
    let succs = self.graph[bb_id].1.clone();

    // Local frontier
    for succ in succs {
      if !self.is_dom(bb_id, succ) {
        self.frontier[bb_id].push(succ);
      }
    }

    // Upward frontier
    let children_num = self.dom_tree[bb_id].len();
    for child_idx in 0..children_num {
      let child = self.dom_tree[bb_id][child_idx];
      self.compute(child);
      let child_frontier_len = self.frontier[child].len();
      for i in 0..child_frontier_len {
        let w = self.frontier[child][i];
        if !self.is_dom(bb_id, w) {
          self.frontier[bb_id].push(w);
        }
      }
    }
  }

  #[inline(always)]
  fn init(&mut self) {
    let n = self.graph.len();
    self.frontier = DomFrontier::with_len(n);
  }

  pub fn build(&mut self) -> DomFrontier {
    self.init();
    if !self.graph.is_empty() {
      self.compute(0);
    }
    std::mem::take(&mut self.frontier)
  }
}

pub struct DomAnalysis<'a> {
  graph: &'a [(Vec<usize>, Vec<usize>)],
}

impl<'a> Analysis for DomAnalysis<'a> {
  type Input = &'a [(Vec<usize>, Vec<usize>)];
  type Output = (DomTree, DomFrontier);

  fn name() -> &'static str {
    "Dominance Analysis"
  }

  fn new(input: Self::Input) -> Self {
    Self { graph: input }
  }

  fn run(&mut self) -> Self::Output {
    let graph = self.graph;
    let mut dom_tree_builder = BuildDomTree::new(graph);
    let dom_trees = dom_tree_builder.build();

    let mut dom_frontier_builder = BuildDomFrontier::new(graph, &dom_trees);
    let dom_frontiers = dom_frontier_builder.build();
    (dom_trees, dom_frontiers)
  }
}

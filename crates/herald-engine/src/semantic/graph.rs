
pub fn sccs(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut t = Tarjan {
        index: vec![usize::MAX; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        counter: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if t.index[v] == usize::MAX {
            t.dfs(adj, v);
        }
    }
    t.out
}

struct Tarjan {
    index: Vec<usize>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    counter: usize,
    out: Vec<Vec<usize>>,
}

impl Tarjan {
    fn dfs(&mut self, adj: &[Vec<usize>], v: usize) {
        self.index[v] = self.counter;
        self.low[v] = self.counter;
        self.counter += 1;
        self.stack.push(v);
        self.on_stack[v] = true;

        for &w in &adj[v] {
            if self.index[w] == usize::MAX {
                self.dfs(adj, w);
                self.low[v] = self.low[v].min(self.low[w]);
            } else if self.on_stack[w] {
                self.low[v] = self.low[v].min(self.index[w]);
            }
        }

        if self.low[v] == self.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = self.stack.pop() {
                self.on_stack[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            self.out.push(comp);
        }
    }
}

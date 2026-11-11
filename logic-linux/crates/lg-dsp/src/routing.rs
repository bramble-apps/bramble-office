//! Signal flow, as Logic's mixer runs it: channel strips feed buses (sends and outputs), aux
//! strips take a bus in and feed further buses or the Stereo Out. `plan` orders the strips so
//! every bus is complete before the aux reading it runs, cuts feedback (a bus feeding itself),
//! and works out plug-in delay compensation: each connection is delayed so that everything
//! meeting at a bus or at the Stereo Out lines up (Logic's "Plug-in Latency Compensation: All").

/// Where a connection goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dest {
    Master,
    Bus(usize),
}

/// One strip: the samples of delay its plug-ins add, the bus it takes in (auxes; None for a
/// track, which makes its own sound), and its connections (output and sends) in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub latency: usize,
    pub input: Option<usize>,
    pub outs: Vec<Dest>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The order to run the strips in.
    pub order: Vec<usize>,
    /// Per strip and connection: samples to delay it by (0 without compensation), or None
    /// where it was cut (it would feed back).
    pub delay: Vec<Vec<Option<usize>>>,
    /// When each strip's output is ready, in samples after the tracks' sound starts.
    pub out_latency: Vec<usize>,
    /// The delay at the Stereo Out (what the whole mix lags by).
    pub total: usize,
}

/// Orders `nodes` and computes each connection's compensating delay (`compensate` false: none,
/// but the order and cuts still apply).
pub fn plan(nodes: &[Node], compensate: bool) -> Plan {
    let n = nodes.len();
    // edges: strip a's connection k to Bus(b) feeds every strip taking b
    let feeds = |a: usize, k: usize| -> Vec<usize> {
        match nodes[a].outs[k] {
            Dest::Bus(b) => (0..n).filter(|&m| nodes[m].input == Some(b)).collect(),
            Dest::Master => Vec::new(),
        }
    };
    // depth-first topological order; a connection to a strip still on the stack is feedback
    let mut cut = vec![Vec::new(); n];
    for (a, nd) in nodes.iter().enumerate() {
        cut[a] = vec![false; nd.outs.len()];
    }
    let mut state = vec![0u8; n];
    let mut post = Vec::with_capacity(n);
    fn visit(a: usize, nodes: &[Node], feeds: &dyn Fn(usize, usize) -> Vec<usize>, state: &mut [u8], cut: &mut [Vec<bool>], post: &mut Vec<usize>) {
        state[a] = 1;
        for k in 0..nodes[a].outs.len() {
            for m in feeds(a, k) {
                match state[m] {
                    0 => visit(m, nodes, feeds, state, cut, post),
                    1 => cut[a][k] = true,
                    _ => {}
                }
            }
        }
        state[a] = 2;
        post.push(a);
    }
    // sources (tracks) first so feedback is cut at the aux that closes the loop
    let mut starts: Vec<usize> = (0..n).filter(|&a| nodes[a].input.is_none()).collect();
    starts.extend((0..n).filter(|&a| nodes[a].input.is_some()));
    for a in starts {
        if state[a] == 0 {
            visit(a, nodes, &feeds, &mut state, &mut cut, &mut post);
        }
    }
    post.reverse();
    let order = post;
    // latencies along the order
    let mut arrive_bus: std::collections::HashMap<usize, usize> = Default::default();
    let mut out_latency = vec![0usize; n];
    for &a in &order {
        let arrive = nodes[a].input.and_then(|b| arrive_bus.get(&b).copied()).unwrap_or(0);
        out_latency[a] = arrive + nodes[a].latency;
        for (k, d) in nodes[a].outs.iter().enumerate() {
            if let (Dest::Bus(b), false) = (d, cut[a][k]) {
                let e = arrive_bus.entry(*b).or_insert(0);
                *e = (*e).max(out_latency[a]);
            }
        }
    }
    let mut total = 0;
    for a in 0..n {
        for (k, d) in nodes[a].outs.iter().enumerate() {
            if *d == Dest::Master && !cut[a][k] {
                total = total.max(out_latency[a]);
            }
        }
    }
    let delay = (0..n)
        .map(|a| {
            nodes[a]
                .outs
                .iter()
                .enumerate()
                .map(|(k, d)| {
                    if cut[a][k] {
                        return None;
                    }
                    if !compensate {
                        return Some(0);
                    }
                    let target = match d {
                        Dest::Master => total,
                        Dest::Bus(b) => arrive_bus.get(b).copied().unwrap_or(0),
                    };
                    Some(target.saturating_sub(out_latency[a]))
                })
                .collect()
        })
        .collect();
    Plan { order, delay, out_latency: if compensate { out_latency } else { vec![0; n] }, total: if compensate { total } else { 0 } }
}

/// A stereo delay line of a fixed length (a compensating connection).
#[derive(Clone, Debug, Default)]
pub struct StereoDelay {
    buf: Vec<(f32, f32)>,
    at: usize,
}

impl StereoDelay {
    pub fn new(samples: usize) -> StereoDelay {
        StereoDelay { buf: vec![(0.0, 0.0); samples], at: 0 }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Delays the block in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.buf.is_empty() {
            return;
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (ol, or) = std::mem::replace(&mut self.buf[self.at], (*l, *r));
            *l = ol;
            *r = or;
            self.at = (self.at + 1) % self.buf.len();
        }
    }

    pub fn clear(&mut self) {
        self.buf.fill((0.0, 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(latency: usize, outs: &[Dest]) -> Node {
        Node { latency, input: None, outs: outs.to_vec() }
    }

    fn aux(latency: usize, input: usize, outs: &[Dest]) -> Node {
        Node { latency, input: Some(input), outs: outs.to_vec() }
    }

    #[test]
    fn tracks_line_up_at_the_stereo_out() {
        // a limiter's 64 samples on track 0: track 1 waits for it
        let p = plan(&[track(64, &[Dest::Master]), track(0, &[Dest::Master])], true);
        assert_eq!(p.total, 64);
        assert_eq!(p.delay, vec![vec![Some(0)], vec![Some(64)]]);
    }

    #[test]
    fn sends_and_aux_returns_line_up() {
        // track 0 (32) sends to bus 0; track 1 (0) sends to bus 0 too; the aux on bus 0 adds 100
        let nodes = [track(32, &[Dest::Master, Dest::Bus(0)]), track(0, &[Dest::Master, Dest::Bus(0)]), aux(100, 0, &[Dest::Master])];
        let p = plan(&nodes, true);
        // the aux runs after both tracks
        let pos = |a| p.order.iter().position(|x| *x == a).unwrap();
        assert!(pos(2) > pos(0) && pos(2) > pos(1));
        // the bus is ready at 32: track 1's send waits 32
        assert_eq!(p.delay[1][1], Some(32));
        assert_eq!(p.delay[0][1], Some(0));
        // the aux is out at 132: the tracks' direct outputs wait for it
        assert_eq!(p.total, 132);
        assert_eq!(p.delay[0][0], Some(100));
        assert_eq!(p.delay[1][0], Some(132));
        assert_eq!(p.delay[2][0], Some(0));
    }

    #[test]
    fn auxes_chain_through_buses() {
        // track → bus 1 → aux A (10) → bus 2 → aux B (5) → out
        let nodes = [aux(5, 2, &[Dest::Master]), aux(10, 1, &[Dest::Bus(2)]), track(0, &[Dest::Bus(1)])];
        let p = plan(&nodes, true);
        assert_eq!(p.order, vec![2, 1, 0]);
        assert_eq!(p.out_latency, vec![15, 10, 0]);
        assert_eq!(p.total, 15);
    }

    #[test]
    fn feedback_is_cut() {
        // aux on bus 0 sends back to bus 0
        let nodes = [track(0, &[Dest::Bus(0)]), aux(0, 0, &[Dest::Master, Dest::Bus(0)])];
        let p = plan(&nodes, true);
        assert_eq!(p.delay[1][1], None);
        assert_eq!(p.delay[1][0], Some(0));
        assert_eq!(p.delay[0][0], Some(0));
    }

    #[test]
    fn no_compensation_keeps_the_order() {
        let nodes = [track(64, &[Dest::Master]), track(0, &[Dest::Master])];
        let p = plan(&nodes, false);
        assert_eq!(p.delay, vec![vec![Some(0)], vec![Some(0)]]);
    }

    #[test]
    fn delay_line_delays() {
        let mut d = StereoDelay::new(3);
        let mut l = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mut r = l;
        d.process(&mut l, &mut r);
        assert_eq!(l, [0.0, 0.0, 0.0, 1.0, 2.0]);
        let mut l2 = [6.0, 7.0];
        let mut r2 = l2;
        d.process(&mut l2, &mut r2);
        assert_eq!(l2, [3.0, 4.0]);
    }
}

//! Bounded A* over actual registered collision tops, with finite steps and drops.
use super::components::Physics;
use crate::scene::Client;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
pub const NODE_BUDGET: usize = 128;
pub const PATH_LENGTH: usize = 32;
type Node = [i64; 3];
fn node(p: [f32; 3]) -> Node {
    [
        p[0].floor() as i64,
        (p[1] * 16.).round() as i64,
        p[2].floor() as i64,
    ]
}
fn point(n: Node) -> [f32; 3] {
    [n[0] as f32 + 0.5, n[1] as f32 / 16., n[2] as f32 + 0.5]
}
fn boxes(client: &Client, p: [i32; 3]) -> Option<&[[f32; 6]]> {
    if !client.scene.inside(p) {
        return None;
    }
    let s = client.scene.engine.as_ref()?;
    Some(s.blocks.get(&p).map_or(&[], |id| s.geometry.collision(*id)))
}
pub fn clear(client: &Client, p: [f32; 3], body: &Physics) -> bool {
    if !super::valid_position(p) {
        return false;
    }
    let half = body.width * 0.5;
    let lo = [p[0] - half + 0.0001, p[1] + 0.0001, p[2] - half + 0.0001];
    let hi = [
        p[0] + half - 0.0001,
        p[1] + body.height - 0.0001,
        p[2] + half - 0.0001,
    ];
    for x in lo[0].floor() as i32..=hi[0].floor() as i32 {
        for z in lo[2].floor() as i32..=hi[2].floor() as i32 {
            for y in lo[1].floor() as i32..=hi[1].floor() as i32 {
                let q = [x, y, z];
                let Some(shapes) = boxes(client, q) else {
                    return false;
                };
                if shapes.iter().any(|b| {
                    (0..3).all(|a| lo[a] < q[a] as f32 + b[a + 3] && hi[a] > q[a] as f32 + b[a])
                }) {
                    return false;
                }
            }
        }
    }
    true
}
pub fn surface(client: &Client, x: f32, z: f32, from_y: f32, body: &Physics) -> Option<[f32; 3]> {
    let mut tops = Vec::new();
    let bx = x.floor() as i32;
    let bz = z.floor() as i32;
    for y in (from_y - 2.5).floor() as i32..=(from_y + 1.).ceil() as i32 {
        let Some(shapes) = boxes(client, [bx, y, bz]) else {
            continue;
        };
        for b in shapes {
            let top = y as f32 + b[4];
            if top <= from_y + 1.01
                && top >= from_y - 2.01
                && x > bx as f32 + b[0] - 0.001
                && x < bx as f32 + b[3] + 0.001
                && z > bz as f32 + b[2] - 0.001
                && z < bz as f32 + b[5] + 0.001
            {
                tops.push(top);
            }
        }
    }
    tops.sort_by(|a, b| (a - from_y).abs().total_cmp(&(b - from_y).abs()));
    tops.dedup();
    tops.into_iter().map(|y| [x, y, z]).find(|p| {
        let s = client.scene.engine.as_ref().unwrap();
        let wet = crate::engine::fluids::amount(s, [bx, p[1].floor() as i32, bz]) > 0;
        !wet && clear(client, *p, body)
    })
}
pub struct Path {
    pub points: Vec<[f32; 3]>,
    pub visited: usize,
    pub complete: bool,
}
pub fn plan(client: &Client, start: [f32; 3], goal: [f32; 3], body: &Physics) -> Path {
    let start_node = node(start);
    let goal_node = node(goal);
    let heuristic = |n: Node| ((n[0] - goal_node[0]).abs() + (n[2] - goal_node[2]).abs()) as u32;
    let mut open = BinaryHeap::new();
    let mut cost = BTreeMap::new();
    let mut parent = BTreeMap::new();
    let mut closed = BTreeSet::new();
    let mut best = start_node;
    let mut complete = false;
    cost.insert(start_node, 0u32);
    open.push(Reverse((heuristic(start_node) * 10, start_node)));
    while let Some(Reverse((_, current))) = open.pop() {
        if !closed.insert(current) {
            continue;
        }
        if closed.len() > NODE_BUDGET {
            break;
        }
        if heuristic(current) < heuristic(best) {
            best = current;
        }
        if heuristic(current) == 0 {
            best = current;
            complete = true;
            break;
        }
        let p = point(current);
        for [x, z] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
            if (current[0] + x - start_node[0]).abs() > 12
                || (current[2] + z - start_node[2]).abs() > 12
            {
                continue;
            }
            let Some(next) = surface(client, p[0] + x as f32, p[2] + z as f32, p[1], body) else {
                continue;
            };
            let next = node(next);
            let candidate = cost[&current] + 10 + ((next[1] - current[1]).abs() / 4) as u32;
            if candidate < *cost.get(&next).unwrap_or(&u32::MAX) {
                cost.insert(next, candidate);
                parent.insert(next, current);
                open.push(Reverse((candidate + heuristic(next) * 10, next)));
            }
        }
    }
    let mut points = Vec::new();
    let mut at = best;
    while at != start_node {
        points.push(point(at));
        let Some(p) = parent.get(&at) else { break };
        at = *p;
    }
    points.reverse();
    points.truncate(PATH_LENGTH);
    Path {
        points,
        visited: closed.len().min(NODE_BUDGET),
        complete,
    }
}

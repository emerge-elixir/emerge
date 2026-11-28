//! Deferred roots in declaration order. Detach the entire deepest FIFO:
//! discovering a submenu must not interrupt its parent's remaining peers.
use super::*;
use pose::Coords;

#[derive(Default, Clone, Copy)]
pub(super) struct Branch {
    pub escape: Coords,
    pub depth: usize,
    pub group: u32,
    pub wheel: u32, // enclosing installed scroll region, zero outside scroll content
}
#[derive(Clone, Copy)]
struct Job {
    root: Key,
    next: u32,
    branch: Branch,
}
// Floats paint locally before inline ink. In a mixed paragraph, park the
// float's submissions until its ordinary ordinal, without revisiting it.
#[derive(Default, Clone, Copy)]
pub(super) struct Chain {
    head: u32,
    tail: u32,
}
impl Chain {
    fn append(&mut self, more: Self, jobs: &mut [Job]) {
        if more.head != 0 {
            if self.tail == 0 {
                self.head = more.head;
            } else {
                jobs[self.tail as usize - 1].next = more.head;
            }
            self.tail = more.tail;
        }
    }
}
#[derive(Default)]
pub(super) struct Bands {
    jobs: Vec<Job>,
    buckets: Vec<[u32; 2]>,
    active: Vec<u64>,
    run: u32,
    capturing: Option<Chain>,
}
impl Bands {
    pub fn reserve(&mut self, n: usize, b: Budget) -> Result<(), String> {
        if n > self.jobs.capacity() {
            pool::reserve(&mut self.jobs, b.capacity(n))?;
        }
        let n = n.min(MAX_DEPTH + 1);
        if n > self.buckets.len() {
            let n = b.capacity(n).min(MAX_DEPTH + 1);
            pool::reserve(&mut self.buckets, n)?;
            pool::reserve(&mut self.active, n.div_ceil(64))?;
            self.buckets.resize(n, [0; 2]);
            self.active.resize(n.div_ceil(64), 0);
        }
        Ok(())
    }
    pub fn begin_float(&mut self) -> Option<Chain> {
        self.capturing.replace(Chain::default())
    }
    pub fn end_float(&mut self, enclosing: Option<Chain>) -> Chain {
        std::mem::replace(&mut self.capturing, enclosing).expect("active float")
    }
    pub fn release_float(&mut self, chain: Chain) {
        if let Some(enclosing) = &mut self.capturing {
            enclosing.append(chain, &mut self.jobs);
        } else {
            let mut link = chain.head;
            while link != 0 {
                let next = std::mem::take(&mut self.jobs[link as usize - 1].next);
                self.queue(link);
                link = next;
            }
        }
    }
    pub fn push(&mut self, root: Key, branch: Branch) {
        self.jobs.push(Job {
            root,
            branch,
            next: 0,
        });
        let link = self.jobs.len() as u32;
        if let Some(chain) = &mut self.capturing {
            chain.append(
                Chain {
                    head: link,
                    tail: link,
                },
                &mut self.jobs,
            );
        } else {
            self.queue(link);
        }
    }
    fn queue(&mut self, link: u32) {
        let d = self.jobs[link as usize - 1].branch.depth;
        let [head, tail] = &mut self.buckets[d];
        if *tail == 0 {
            *head = link;
            self.active[d / 64] |= 1 << (d % 64);
        } else {
            self.jobs[*tail as usize - 1].next = link;
        }
        *tail = link;
    }
    pub fn next(&mut self) -> Option<(Key, Branch, bool)> {
        let new_band = self.run == 0;
        if new_band {
            let i = self.active.iter().rposition(|w| *w != 0)?;
            let bit = 63 - self.active[i].leading_zeros() as usize;
            let d = 64 * i + bit;
            self.active[i] &= !(1 << bit);
            self.run = std::mem::take(&mut self.buckets[d])[0];
        }
        let job = self.jobs[self.run as usize - 1];
        self.run = job.next;
        Some((
            job.root,
            Branch {
                depth: job.branch.depth + 1,
                ..job.branch
            },
            new_band,
        ))
    }
    pub fn finish(&mut self) {
        debug_assert!(self.capturing.is_none());
        self.jobs.clear();
    }
}

// One saved path node per enclosing opacity owner, not one list per job.
// Delimiters never cross a band. Consecutive jobs within one band share the
// longest common prefix, so overlapping sibling menus composite together.
struct Path {
    parent: u32,
    depth: usize,
    alpha: u8,
    start: usize,
}
#[derive(Default)]
pub(super) struct Groups {
    paths: Vec<Path>,
    active: u32,
}
impl Groups {
    pub fn reserve(&mut self, n: usize, b: Budget) -> Result<(), String> {
        if n > self.paths.capacity() {
            pool::reserve(&mut self.paths, b.capacity(n))?;
        }
        Ok(())
    }
    fn depth(&self, id: u32) -> usize {
        if id == 0 {
            0
        } else {
            self.paths[id as usize - 1].depth
        }
    }
    pub fn child(&mut self, parent: u32, alpha: u8) -> u32 {
        self.paths.push(Path {
            depth: self.depth(parent) + 1,
            parent,
            alpha,
            start: 0,
        });
        self.paths.len() as u32
    }
    fn open(&mut self, draw: &mut Drawing, from: u32, to: u32) {
        if to != from {
            let i = to as usize - 1;
            self.open(draw, from, self.paths[i].parent);
            self.paths[i].start = draw.begin_group(self.paths[i].alpha);
        }
    }
    pub fn switch(&mut self, draw: &mut Drawing, vp: Rect, to: u32) {
        let mut common = to;
        while self.depth(common) > self.depth(self.active) {
            common = self.paths[common as usize - 1].parent;
        }
        while self.active != common {
            if self.depth(self.active) == self.depth(common) {
                common = self.paths[common as usize - 1].parent;
            }
            let p = &self.paths[self.active as usize - 1];
            draw.end_group(p.start, vp);
            self.active = p.parent;
        }
        self.open(draw, common, to);
        self.active = to;
    }
    pub fn finish(&mut self) {
        debug_assert_eq!(self.active, 0);
        self.paths.clear();
    }
}

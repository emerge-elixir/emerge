//! A frame changes one content edge without copying a directory of other
//! scrollports. The compressed bit trie has one leaf for a one-port scene.
use super::*;
use slotmap::Key as _;
type Link = Option<Ref<Edge>>;
#[derive(Clone)]
enum Program {
    Window(Ref<Window>),
    Owner(Ref<Local>),
}
impl Program {
    fn key(&self) -> Key {
        match self {
            Self::Window(w) => w.key.unwrap(),
            Self::Owner(p) => p.key.unwrap().0,
        }
    }
}
#[derive(Default)]
enum Edge {
    #[default]
    Empty,
    Tip(Program),
    Fork {
        bit: u64,
        sample: u64,
        children: [Link; 2],
    },
}
impl Recycle for Edge {
    fn recycle(&mut self) {
        *self = Self::Empty
    }
}
impl Edge {
    fn sample(&self) -> u64 {
        match self {
            Self::Tip(w) => w.key().data().as_ffi(),
            Self::Fork { sample, .. } => *sample,
            _ => unreachable!(),
        }
    }
}
fn put(stock: &Stock<Edge>, mut root: Link, w: Program) -> Link {
    let k = w.key().data().as_ffi();
    let write = || stock.issue(|p| *p = Edge::Tip(w.clone()));
    let Some(a) = root.take() else {
        return Some(write());
    };
    let sample = a.sample();
    let diff = k ^ sample;
    let top = if diff == 0 {
        0
    } else {
        1u64 << (63 - diff.leading_zeros())
    };
    if let Edge::Fork { bit, children, .. } = &*a
        && *bit >= top
    {
        let mut children = children.clone();
        let i = usize::from(k & bit != 0);
        children[i] = put(stock, children[i].take(), w);
        return Some(stock.issue(|p| {
            *p = Edge::Fork {
                bit: *bit,
                sample,
                children,
            }
        }));
    }
    let fresh = Some(write());
    if top == 0 {
        return fresh;
    }
    let children = if k & top != 0 {
        [Some(a), fresh]
    } else {
        [fresh, Some(a)]
    };
    Some(stock.issue(|p| {
        *p = Edge::Fork {
            bit: top,
            sample,
            children,
        }
    }))
}
#[derive(Default, Clone)]
pub(in crate::tree::paint) struct Tables {
    root: [Link; 2],
    pub counts: [usize; 3],
    pub markers: [usize; 2],
    edges: [usize; 2],
    used: [usize; 2],
    pub picture: u64,
    version: u64,
    session: Option<Arc<()>>,
}
impl Tables {
    pub fn same_picture(&self, b: &Self) -> bool {
        (self.root[0].is_none() && b.root[0].is_none())
            || (self.picture == b.picture
                && self
                    .session
                    .as_ref()
                    .zip(b.session.as_ref())
                    .is_some_and(|(x, y)| Arc::ptr_eq(x, y)))
    }
    pub const EMPTY: Self = Self {
        root: [None, None],
        counts: [0; 3],
        markers: [0; 2],
        edges: [0; 2],
        used: [0; 2],
        picture: 0,
        version: 0,
        session: None,
    };
    pub fn get(&self, k: Key, side: usize) -> Option<&Window> {
        match self.program(k, side)? {
            Program::Window(w) => Some(w),
            _ => None,
        }
    }
    pub fn owner(&self, k: Key, side: usize) -> Option<&Local> {
        match self.program(k, side)? {
            Program::Owner(p) => Some(p),
            _ => None,
        }
    }
    fn program(&self, k: Key, side: usize) -> Option<&Program> {
        let bits = k.data().as_ffi();
        let mut edge = self.root[side].as_deref()?;
        loop {
            match edge {
                Edge::Tip(w) => return (w.key() == k).then_some(w),
                Edge::Fork { bit, children, .. } => {
                    edge = children[usize::from(bits & bit != 0)].as_deref()?
                }
                Edge::Empty => return None,
            }
        }
    }
}
impl Packed<0> for Tables {
    fn capacities(&self) -> [usize; 0] {
        []
    }
    fn reserve(&mut self, _: [usize; 0]) -> Result<(), String> {
        Ok(())
    }
    fn clear(&mut self) {
        *self = Self::EMPTY
    }
}
#[derive(Default)]
pub(in crate::tree::paint) struct Bindings {
    bank: Option<Buffers<Tables, 0>>,
    nodes: Stock<Edge>,
    windows: Stock<Window>,
    owners: Stock<Local>,
    pub current: Tables,
    previous: Tables,
    matched: bool,
    matched_rows: usize,
    matched_picture: bool,
    serial: u64,
}
impl Bindings {
    pub fn charge(sites: Sites, readers: u128, b: Budget) -> u128 {
        let ports = sites.ports as u128;
        let owners = sites.instances() as u128;
        Stock::<Edge>::charge((readers + 4) * 4 * (ports + owners), 0, b)
            + Stock::<Window>::charge((readers + 4) * 2 * ports, 0, b)
            + Stock::<Local>::charge((readers + 3) * 2 * owners, 0, b)
    }
    pub fn reserve(&mut self, sites: Sites, readers: usize, b: Budget) -> Result<(), String> {
        let ports = sites.ports;
        let owners = sites.instances();
        self.owners
            .reserve(b.capacity((readers + 3) * 2 * owners), || {
                Ok(Local::default())
            })?;
        self.bank.get_or_insert_with(|| Buffers::new(readers));
        self.current.session.get_or_insert_with(Default::default);
        self.windows
            .reserve(b.capacity((readers + 4) * 2 * ports), || {
                Ok(Window::default())
            })?;
        self.nodes
            .reserve(b.capacity((readers + 4) * 4 * (ports + owners)), || {
                Ok(Edge::Empty)
            })
    }
    pub fn installed(&self) -> Option<&Arc<Tables>> {
        self.bank.as_ref().map(Buffers::current)
    }
    pub fn available(&self) -> bool {
        self.bank
            .as_ref()
            .is_none_or(|b| b.current().version == self.current.version || b.available())
    }
    pub fn reclaim(&mut self) {
        if let Some(b) = &mut self.bank {
            b.reclaim()
        }
    }
    pub fn install(&mut self) {
        if let Some(b) = &mut self.bank
            && b.current().version != self.current.version
        {
            b.build = self.current.clone();
            b.publish();
        }
    }
    fn tick(&mut self, side: usize) {
        self.serial += 1;
        self.current.version = self.serial;
        if side == 0 {
            self.current.picture = self.serial;
        }
    }
    fn insert(&mut self, w: &Window, s: usize, counted: bool) {
        let w = self.windows.issue(|v| {
            *v = w.clone();
            v.counted = counted;
            v.root[1 - s] = None;
            v.shell[1 - s] = Local::default();
            v.ranges[1 - s] = Selection::default();
        });
        if counted {
            self.current.used[s] += 1;
            if s == 0 {
                self.current.counts[0] += w.counts[0];
                self.current.counts[1] += w.counts[1];
            } else {
                self.current.counts[2] += w.counts[2];
            }
        }
        self.current.edges[s] += 1;
        self.current.root[s] = put(&self.nodes, self.current.root[s].take(), Program::Window(w));
    }
    pub fn update(&mut self, w: &Window) {
        for s in 0..2 {
            if let Some(old) = self.current.get(w.key.unwrap(), s)
                && !old.same_version(w, s)
            {
                let picture_changed = s == 0 && !old.same(w, s);
                let counts = old.counts;
                let counted = old.counted;
                if counted {
                    self.current.used[s] -= 1;
                    if s == 0 {
                        self.current.counts[0] -= counts[0];
                        self.current.counts[1] -= counts[1];
                    } else {
                        self.current.counts[2] -= counts[2];
                    }
                }
                self.current.edges[s] -= 1;
                self.insert(w, s, counted);
                self.tick(usize::from(!picture_changed));
            }
        }
    }
    pub fn begin(&mut self, s: usize) {
        self.previous = self.current.clone();
        self.current.root[s] = None;
        self.current.markers[s] = 0;
        self.current.edges[s] = 0;
        self.current.used[s] = 0;
        self.matched_rows = 0;
        if s == 0 {
            self.current.counts[..2].fill(0);
        } else {
            self.current.counts[2] = 0;
        }
        self.matched = true;
        self.matched_picture = true;
    }
    pub fn include_owner(&mut self, p: &Local, s: usize) {
        let key = p.key.unwrap().0;
        let old = self.previous.program(key, s);
        let saved = match old {
            Some(Program::Owner(r)) if r.same_program(p) => Some(r.clone()),
            _ => None,
        };
        let same = saved.is_some();
        self.matched &= same;
        self.matched_picture &= same;
        let record = saved.unwrap_or_else(|| self.owners.issue(|a| *a = p.clone()));
        self.current.root[s] = put(
            &self.nodes,
            self.current.root[s].take(),
            Program::Owner(record),
        );
        self.current.edges[s] += 1;
        self.current.markers[s] += 1;
        for (n, c) in self.current.counts.iter_mut().zip(p.counts()) {
            *n += c;
        }
    }
    pub fn mark(&mut self, s: usize) {
        self.current.markers[s] += 1;
    }
    pub fn enable_rows(&mut self, w: &Window, s: usize) {
        let k = w.key.unwrap();
        debug_assert!(!self.current.get(k, s).unwrap().counted);
        self.matched_rows += usize::from(self.previous.get(k, s).is_some_and(|a| a.counted));
        self.current.edges[s] -= 1;
        self.insert(w, s, true);
    }
    pub fn include(&mut self, w: &Window, s: usize, counted: bool) {
        let old = self.previous.get(w.key.unwrap(), s);
        self.matched &= old.is_some_and(|old| old.same_version(w, s));
        self.matched_picture &= old.is_some_and(|old| old.same(w, s));
        self.matched_rows += usize::from(counted && old.is_some_and(|a| a.counted));
        self.insert(w, s, counted);
    }
    pub fn end(&mut self, s: usize) {
        let same_structure = self.previous.edges[s] == self.current.edges[s]
            && self.previous.used[s] == self.matched_rows
            && self.current.used[s] == self.matched_rows;
        if self.matched && same_structure {
            self.current.root[s] = self.previous.root[s].take();
        } else {
            let picture = self.matched_picture && same_structure;
            self.tick(if picture { 1 } else { s });
        }
        self.previous = Tables::EMPTY;
    }
}

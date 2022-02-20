use alloc::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::{Pid, Process};

pub struct ChildrenSet {
    alive: BTreeMap<Pid, Arc<Process>>,
    zombie: BTreeMap<Pid, Arc<Process>>,
    zombie_pending: BTreeSet<Pid>, // alive + zombie_pending => zombie
}

impl Default for ChildrenSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ChildrenSet {
    pub fn new() -> Self {
        Self {
            alive: BTreeMap::new(),
            zombie: BTreeMap::new(),
            zombie_pending: BTreeSet::new(),
        }
    }
    pub fn alive_no_find(&self, pid: Pid) -> bool {
        self.alive.get(&pid).is_none()
    }
    pub fn zombie_no_find(&self, pid: Pid) -> bool {
        self.zombie.get(&pid).is_none()
    }
    pub fn zombie_pending_no_find(&self, pid: Pid) -> bool {
        self.zombie_pending.get(&pid).is_none()
    }
    pub fn have_child_of(&mut self, pid: Pid) -> bool {
        !self.alive_no_find(pid) || !self.zombie_no_find(pid) || !self.zombie_pending_no_find(pid)
    }
    pub fn no_children(&mut self) -> bool {
        self.alive.is_empty() && self.zombie.is_empty() && self.zombie_pending.is_empty()
    }

    /// check zombies
    pub fn push_child(&mut self, child: Arc<Process>) {
        if self.zombie_pending.remove(&child.pid()) {
            self.push_zombie_child(child)
        } else {
            self.push_alive_child(child)
        }
    }
    pub fn push_alive_child(&mut self, child: Arc<Process>) {
        let pid = child.pid();
        debug_check!(self.zombie_no_find(pid));
        debug_check!(self.zombie_pending_no_find(pid));
        match self.alive.insert(pid, child) {
            Some(_) => panic!(),
            None => (),
        }
    }
    pub fn push_zombie_child(&mut self, child: Arc<Process>) {
        let pid = child.pid();
        debug_check!(self.alive_no_find(pid));
        debug_check!(self.zombie_pending_no_find(pid));
        match self.zombie.insert(child.pid(), child) {
            Some(_) => panic!(),
            None => (),
        }
    }
    pub fn become_zombie(&mut self, pid: Pid) {
        if let Some(child) = self.alive.remove(&pid) {
            self.push_zombie_child(child);
            return;
        }
        debug_check!(self.zombie_no_find(pid));
        if !self.zombie_pending.insert(pid) {
            panic!()
        }
    }
    pub fn try_remove_zombie(&mut self, pid: Pid) -> Option<Arc<Process>> {
        self.zombie.remove(&pid)
    }
    pub fn try_remove_zombie_any(&mut self) -> Option<Arc<Process>> {
        self.zombie.pop_first().map(|(_pid, ptr)| ptr)
    }
    pub fn append(&mut self, mut src: Self) {
        self.alive.append(&mut src.alive);
        self.zombie.append(&mut src.zombie);
        // self.zombie_pending.append(&mut src.zombie_pending);
        let zombie_pending = &mut src.zombie_pending;
        zombie_pending.append(&mut self.zombie_pending);
        while let Some(pid) = zombie_pending.pop_first() {
            self.become_zombie(pid);
        }
    }
    pub fn alive_iter(&self) -> impl Iterator<Item = (&Pid, &Arc<Process>)> + '_ {
        self.alive.iter()
    }
}

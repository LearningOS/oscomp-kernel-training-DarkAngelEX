use crate::tools::CID;

use super::common::Fat32Common;

pub struct Fat32File {
    common: Fat32Common,
}

impl Fat32File {
    pub fn new(cid: CID) -> Self {
        Self {
            common: Fat32Common::new(cid),
        }
    }
}

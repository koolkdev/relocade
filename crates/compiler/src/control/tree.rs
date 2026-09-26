//! Region ancestry shared by value rewriting and placement.

use std::collections::HashMap;

use super::{Region, Site};
use crate::Operation;

struct RegionInfo<'a> {
    region: &'a Region,
    parent: Option<Site>,
    depth: usize,
}

pub(crate) struct RegionTree<'a>(HashMap<usize, RegionInfo<'a>>);

impl<'a> RegionTree<'a> {
    pub(crate) fn new(root: &'a Region) -> Self {
        let mut regions = HashMap::new();
        let mut pending = vec![(root, None, 0)];
        while let Some((region, parent, depth)) = pending.pop() {
            for (index, operation) in region.operations.iter().enumerate() {
                for child in operation.children() {
                    pending.push((
                        child,
                        Some(Site {
                            region: region.id,
                            index,
                        }),
                        depth + 1,
                    ));
                }
            }
            regions.insert(
                region.id,
                RegionInfo {
                    region,
                    parent,
                    depth,
                },
            );
        }
        Self(regions)
    }

    pub(crate) fn region(&self, id: usize) -> &'a Region {
        self.0[&id].region
    }

    pub(crate) fn parent(&self, id: usize) -> Option<Site> {
        self.0[&id].parent
    }

    // Detached construction values can still name a discarded operation.
    pub(crate) fn operation(&self, site: Site) -> Option<&'a Operation> {
        self.0
            .get(&site.region)
            .map(|info| &info.region.operations[site.index])
    }

    /// Lift both sites into their nearest common region, preserving input order.
    pub(crate) fn common_region(&self, mut a: Site, mut b: Site) -> (Site, Site) {
        while self.0[&a.region].depth > self.0[&b.region].depth {
            a = self.parent(a.region).expect("a deeper region has a parent");
        }
        while self.0[&b.region].depth > self.0[&a.region].depth {
            b = self.parent(b.region).expect("a deeper region has a parent");
        }
        while a.region != b.region {
            a = self
                .parent(a.region)
                .expect("distinct regions have parents");
            b = self
                .parent(b.region)
                .expect("distinct regions have parents");
        }
        (a, b)
    }
}

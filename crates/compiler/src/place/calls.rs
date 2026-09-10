//! One placement for every demanded result of an invocation.
use super::{demand, Demand, Planner, Point};
use crate::{control::Site, effects::Effects};

impl Planner<'_> {
    pub(super) fn place_call(&mut self, site: Site) {
        let body = self.body;
        let tree = &self.tree;
        let effects = self.effects;
        let demands = &mut self.demands;
        let captures = &mut self.capture_points;
        let saved = &mut self.saved;
        let (invocation, outputs) = body.call(site);
        let summary = &effects[invocation.target.0];
        let mut combined = summary
            .must_execute()
            .then(|| Demand::at(Point::main(site)));
        for &output in outputs {
            if let Some(use_) = &demands[output] {
                for &point in &use_.points {
                    if let Some(combined) = &mut combined {
                        combined.include(point, tree);
                    } else {
                        combined = Some(Demand::at(point));
                    }
                }
            }
        }
        let Some(use_) = combined else { return };
        let mut anchor = use_.first;
        if summary.must_execute() {
            anchor = Point::main(site);
        } else if let Effects::Known { reads, .. } = summary {
            if tree.clobbers(
                site,
                anchor.site,
                |location| {
                    reads
                        .iter()
                        .any(|read| read.overlaps_location(location, body))
                },
                |target| effects[target.0].writes_reads(reads),
            ) {
                anchor = Point::main(site);
            }
        }
        let capture = !use_.at_first || anchor != use_.first;
        if let [output] = outputs {
            // Preserve scalar stack placement, including unused effectful calls.
            saved[*output] = use_.points.len() > 1 || capture;
        } else {
            // A call returns every component together. Keep only live components;
            // their individual consumers may follow different paths or stores.
            for &output in outputs {
                saved[output] = demands[output].is_some();
            }
        }
        if capture {
            let output = outputs
                .iter()
                .copied()
                .find(|&output| demands[output].is_some())
                .expect("a call captured away from its site has a demanded result");
            captures[output].push(anchor);
        }
        for &argument in &invocation.arguments {
            demand(body, tree, demands, argument, anchor);
        }
    }
}

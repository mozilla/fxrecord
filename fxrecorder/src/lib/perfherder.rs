// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::analysis::VisualMetrics;
use serde_json::{json, Value};

/// Generate a JSON blob containing the performance metrics for Perfherder.
pub fn generate_perfherder_metrics(metrics: &VisualMetrics) -> Value {
    json!({
      "application": {
        "name": "firefox",
      },
      "framework": {
        "name": "fxrecord",
      },
      "suites": [
        {
          "name": "firstrun",
          "subtests": [
            {
              "name": "SpeedIndex",
              "value": metrics.speed_index,
              "unit": "ms * %",
              "lowerIsBetter": true,
              "shouldAlert": true,
            },
            {
              "name": "FirstVisualChange",
              "value": metrics.first_visual_change,
              "unit": "ms",
              "lowerIsBetter": true,
              "shouldAlert": true,
            },
            {
              "name": "LastVisualChange",
              "value": metrics.last_visual_change,
              "unit": "ms",
              "lowerIsBetter": true,
              "shouldAlert": true,
            }
          ]
        }
      ],
    })
}

// This file is part of faster, the SIMD library for humans.
// Copyright 2017 Adam Niederer <adam.niederer@gmail.com>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// macro_rules! impl_cast {
//     ($trait:path, $from:ty, $to:ty, $name:ident, $rsname:ident) => (
//         impl $trait for $from {
//             type Cast = $to;

//             #[inline(always)]
//             fn $name(self) -> Self::Cast {
//                 self.$rsname()
//             }
//         }
//     );
// }
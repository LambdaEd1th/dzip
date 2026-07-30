use super::common::{CommonReference, CommonRoot, CommonSegment, CommonSelection};
use crate::chunk::encode_recent_distance;
use crate::matchfinder::{LazyLzParser, LzDecision, MatchCost, MatchScoring, common_match_hash};
use crate::{DzipError, RangeSettings, Result};
use std::collections::HashMap;

pub(crate) fn find_common_references(
    inputs: &[Vec<u8>],
    settings: RangeSettings,
    preprocess: bool,
    minimum_match: usize,
    maximum_match: usize,
    trim_reference_factor: i32,
) -> Result<(Vec<CommonSegment>, Vec<Vec<CommonReference>>)> {
    let coverage: Vec<Vec<bool>> = if preprocess {
        inputs
            .iter()
            .map(|input| local_match_coverage(input, settings))
            .collect::<Result<Vec<_>>>()?
    } else {
        inputs
            .iter()
            .map(|input| vec![false; input.len()])
            .collect()
    };
    let mut flags: Vec<Vec<u8>> = coverage
        .iter()
        .map(|covered| {
            covered
                .iter()
                .map(|&covered| if covered { 8 } else { 0 })
                .collect()
        })
        .collect();
    let mut chains: Vec<HashMap<u32, Vec<usize>>> =
        (0..inputs.len()).map(|_| HashMap::new()).collect();
    let mut skip_until = vec![0usize; inputs.len()];
    let mut selections = Vec::<CommonSelection>::new();
    let scan_end = inputs
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .saturating_sub(minimum_match);

    for position in 0..scan_end {
        for target_file in 0..inputs.len() {
            let target = &inputs[target_file];
            if position < skip_until[target_file]
                || position.saturating_add(minimum_match) >= target.len()
            {
                continue;
            }

            if flags[target_file][position] & 0xf7 != 0 {
                let run = flags[target_file][position..]
                    .iter()
                    .take_while(|&&flag| flag & 0xf7 != 0)
                    .count();
                skip_until[target_file] = position.saturating_add(run);
                continue;
            }

            if let Some(distance) = flags[target_file][position..position + minimum_match]
                .iter()
                .position(|&flag| flag & 1 != 0)
            {
                if distance != 0 {
                    skip_until[target_file] = position.saturating_add(distance);
                    continue;
                }
            }

            let Some(hash) = common_match_hash(target, position, minimum_match) else {
                continue;
            };
            let mut best_score = 0i64;
            let mut best = None;

            for source_file in 0..inputs.len() {
                let source = &inputs[source_file];
                let Some(candidates) = chains[source_file].get(&hash) else {
                    continue;
                };
                let mut previous_source = None;
                let mut previous_length = 0usize;
                let mut previous_score = 0i64;
                let mut repeating_run = false;
                for &source_position in candidates.iter().rev() {
                    if flags[source_file][source_position] & 4 != 0 {
                        continue;
                    }
                    if source.get(source_position..source_position + minimum_match)
                        != target.get(position..position + minimum_match)
                    {
                        continue;
                    }

                    let incremental = previous_source == Some(source_position.saturating_add(1));
                    if incremental && !repeating_run {
                        repeating_run = true;
                        for index in 2..previous_length + 1 {
                            if source.get(source_position + index)
                                != source.get(source_position + 1)
                            {
                                repeating_run = false;
                                break;
                            }
                        }
                    }

                    let (length, score) = if incremental && repeating_run {
                        if target.get(position + previous_length) == target.get(position) {
                            (
                                previous_length + 1,
                                previous_score
                                    + 8
                                    + 8 * i64::from(flags[source_file][source_position] & 1 != 0),
                            )
                        } else {
                            (
                                previous_length,
                                previous_score
                                    - 8 * i64::from(
                                        flags[source_file][source_position + previous_length] & 1
                                            != 0,
                                    )
                                    + 8 * i64::from(flags[source_file][source_position] & 1 != 0),
                            )
                        }
                    } else {
                        repeating_run = false;
                        let mut limit = usize::min(
                            maximum_match,
                            usize::min(target.len() - position, source.len() - source_position),
                        );
                        if source_file == target_file {
                            limit = usize::min(limit, position.abs_diff(source_position));
                        }
                        let mut length = minimum_match;
                        while length < limit
                            && target[position + length] == source[source_position + length]
                        {
                            length += 1;
                        }
                        if let Some(excluded) = flags[source_file]
                            [source_position..source_position + length]
                            .iter()
                            .position(|&flag| flag & 4 != 0)
                        {
                            length = excluded;
                        }
                        if length < minimum_match {
                            continue;
                        }
                        let already_common = flags[source_file]
                            [source_position..source_position + length]
                            .iter()
                            .filter(|&&flag| flag & 1 != 0)
                            .count();
                        let has_existing_start = flags[source_file][source_position] & 2 != 0;
                        (
                            length,
                            common_match_score(length, already_common, has_existing_start),
                        )
                    };
                    previous_source = Some(source_position);
                    previous_length = length;
                    previous_score = score;
                    if score > best_score {
                        best_score = score;
                        best = Some((source_file, source_position, length));
                    }
                }
            }

            let Some((source_file, source_position, length)) = best else {
                continue;
            };
            let local_run = flags[target_file][position..]
                .iter()
                .take_while(|&&flag| flag & 8 != 0)
                .count();
            if length <= local_run {
                skip_until[target_file] = position.saturating_add(local_run);
                continue;
            }

            flags[target_file][position..position + length]
                .iter_mut()
                .for_each(|flag| *flag |= 4);
            flags[source_file][source_position..source_position + length]
                .iter_mut()
                .for_each(|flag| *flag |= 1);
            flags[source_file][source_position] |= 2;
            skip_until[target_file] = position.saturating_add(length);
            selections.push(CommonSelection {
                target_file,
                target_position: position,
                source_file,
                source_position,
                length,
            });
        }

        for (file_index, input) in inputs.iter().enumerate() {
            if position.saturating_add(minimum_match) <= input.len() {
                if let Some(hash) = common_match_hash(input, position, minimum_match) {
                    chains[file_index].entry(hash).or_default().push(position);
                }
            }
        }
    }

    if trim_reference_factor != 0 {
        trim_common_selections(&mut selections, &coverage, trim_reference_factor);
    }

    // sub_49B800 turns overlapping source records into one root interval and
    // keeps every distinct source start as a restart point within that root.
    let mut roots = Vec::<CommonRoot>::new();
    for source_file in 0..inputs.len() {
        let mut ranges: Vec<_> = selections
            .iter()
            .filter(|selection| selection.source_file == source_file)
            .map(|selection| {
                (
                    selection.source_position,
                    selection.source_position + selection.length,
                )
            })
            .collect();
        ranges.sort_unstable();
        for (start, end) in ranges {
            let mut merged = false;
            if let Some(root) = roots.last_mut() {
                if root.source_file == source_file && start < root.end {
                    root.end = root.end.max(end);
                    root.boundaries.push(start);
                    merged = true;
                }
            }
            if !merged {
                roots.push(CommonRoot {
                    source_file,
                    start,
                    end,
                    boundaries: vec![start],
                });
            }
        }
    }
    for root in &mut roots {
        root.boundaries.sort_unstable();
        root.boundaries.dedup();
    }
    // Match sub_49BDB0's deterministic seed: the first root is taken from
    // file zero, then roots from files it references are placed next. The
    // remaining roots retain file/offset order.
    order_common_roots(&mut roots, &selections, inputs.len());

    let mut segments = Vec::<CommonSegment>::new();
    let mut boundary_to_segment = HashMap::<(usize, usize), usize>::new();
    for root in roots {
        for (boundary_index, &start) in root.boundaries.iter().enumerate() {
            let end = root
                .boundaries
                .get(boundary_index + 1)
                .copied()
                .unwrap_or(root.end);
            let segment = segments.len();
            boundary_to_segment.insert((root.source_file, start), segment);
            let raw = inputs[root.source_file][start..end].to_vec();
            segments.push(CommonSegment {
                source_file: root.source_file,
                source_position: start,
                decision_len: raw.len(),
                raw,
                lookahead: inputs[root.source_file][start..root.end].to_vec(),
                allow_position_zero: boundary_index == 0,
                emit_end: boundary_index + 1 < root.boundaries.len(),
                trailing_literal: None,
                encoded: Vec::new(),
                target: 0,
            });
        }
    }
    // The common-tree traversal in dzip 1.1.3 visits the final source byte
    // twice before writing the last END marker. It is harmless to decoding
    // (no later source begins there), but it participates in both static
    // frequency analysis and the final range stream.
    if let Some(last) = segments.last_mut() {
        if let Some(&final_byte) = last.raw.last() {
            last.trailing_literal = Some(final_byte);
            last.emit_end = true;
        }
    }

    let mut references = vec![Vec::new(); inputs.len()];
    for selection in selections {
        let Some(&segment) =
            boundary_to_segment.get(&(selection.source_file, selection.source_position))
        else {
            return Err(DzipError::InvalidDz(
                "internal COMBUF source boundary is missing".to_string(),
            ));
        };
        references[selection.target_file].push(CommonReference {
            position: selection.target_position,
            length: selection.length,
            segment,
        });
    }
    for chunk_references in &mut references {
        chunk_references.sort_by_key(|reference| reference.position);
    }

    Ok((segments, references))
}

fn common_match_score(length: usize, already_common: usize, has_existing_start: bool) -> i64 {
    let length_code_cost = if length < 13 {
        length + 3
    } else {
        let mut value = (length - 13) >> 2;
        let mut cost = 18usize;
        while value != 0 {
            cost += 1;
            value >>= 2;
        }
        cost
    };
    let reference_cost = length_code_cost + if has_existing_start { 9 } else { 17 };
    8 * (length + already_common) as i64
        - reference_cost as i64
        - if has_existing_start { 0 } else { 40 }
}

fn order_common_roots(
    roots: &mut Vec<CommonRoot>,
    selections: &[CommonSelection],
    file_count: usize,
) {
    #[derive(Clone, Copy)]
    enum Record {
        Boundary { root: usize },
        Reference { selection: usize },
    }

    fn record_root(
        record: Record,
        roots: &[CommonRoot],
        selections: &[CommonSelection],
        source_roots: &HashMap<(usize, usize), usize>,
    ) -> Option<usize> {
        match record {
            Record::Boundary { root } => Some(root),
            Record::Reference { selection } => {
                let selection = selections.get(selection)?;
                source_roots
                    .get(&(selection.source_file, selection.source_position))
                    .copied()
                    .filter(|&root| root < roots.len())
            }
        }
    }

    let mut source_roots = HashMap::<(usize, usize), usize>::new();
    let mut records = vec![Vec::<(usize, Record)>::new(); file_count];
    for (root_index, root) in roots.iter().enumerate() {
        for &boundary in &root.boundaries {
            source_roots.insert((root.source_file, boundary), root_index);
            records[root.source_file].push((boundary, Record::Boundary { root: root_index }));
        }
    }
    for (selection_index, selection) in selections.iter().enumerate() {
        records[selection.target_file].push((
            selection.target_position,
            Record::Reference {
                selection: selection_index,
            },
        ));
    }
    for file_records in &mut records {
        file_records.sort_by_key(|&(position, _)| position);
    }

    let mut reference_record_positions = vec![None; selections.len()];
    let mut last_boundary_positions = vec![None; roots.len()];
    for (file_index, file_records) in records.iter().enumerate() {
        for (record_index, &(_, record)) in file_records.iter().enumerate() {
            match record {
                Record::Boundary { root } => {
                    last_boundary_positions[root] = Some((file_index, record_index));
                }
                Record::Reference { selection } => {
                    reference_record_positions[selection] = Some((file_index, record_index));
                }
            }
        }
    }

    let mut attached = vec![Vec::<usize>::new(); roots.len()];
    let mut selection_order: Vec<_> = (0..selections.len()).collect();
    selection_order.sort_by_key(|&index| {
        (
            selections[index].target_file,
            selections[index].target_position,
        )
    });
    for selection_index in selection_order {
        let selection = selections[selection_index];
        let Some(&root) = source_roots.get(&(selection.source_file, selection.source_position))
        else {
            continue;
        };
        if selection.source_position + selection.length == roots[root].end {
            attached[root].push(selection_index);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        root: usize,
        roots: &[CommonRoot],
        selections: &[CommonSelection],
        records: &[Vec<(usize, Record)>],
        source_roots: &HashMap<(usize, usize), usize>,
        last_boundary_positions: &[Option<(usize, usize)>],
        reference_record_positions: &[Option<(usize, usize)>],
        attached: &[Vec<usize>],
        visited: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        if visited[root] {
            return;
        }
        visited[root] = true;
        order.push(root);

        let mut candidates = Vec::<usize>::new();
        if let Some((file, position)) = last_boundary_positions[root] {
            if let Some(&(_, record)) = records[file].get(position + 1) {
                if let Some(candidate) = record_root(record, roots, selections, source_roots) {
                    if !visited[candidate] {
                        candidates.push(candidate);
                    }
                }
            }
        }
        for &selection in &attached[root] {
            if candidates.len() >= 8 {
                break;
            }
            let Some((file, position)) = reference_record_positions[selection] else {
                continue;
            };
            let Some(&(_, record)) = records[file].get(position + 1) else {
                continue;
            };
            if let Some(candidate) = record_root(record, roots, selections, source_roots) {
                if !visited[candidate] {
                    candidates.push(candidate);
                }
            }
        }

        while !candidates.is_empty() {
            let mut counts = HashMap::<usize, usize>::new();
            let mut most_common = (0usize, 0usize);
            for &candidate in &candidates {
                let count = counts.entry(candidate).or_default();
                *count += 1;
                if *count > most_common.1 {
                    most_common = (candidate, *count);
                }
            }
            let chosen = if most_common.1 > 1 {
                most_common.0
            } else {
                *candidates
                    .iter()
                    .min_by_key(|&&candidate| roots[candidate].end - roots[candidate].start)
                    .expect("non-empty candidates")
            };
            let index = candidates
                .iter()
                .position(|&candidate| candidate == chosen)
                .expect("chosen candidate exists");
            candidates.remove(index);
            visit(
                chosen,
                roots,
                selections,
                records,
                source_roots,
                last_boundary_positions,
                reference_record_positions,
                attached,
                visited,
                order,
            );
        }
    }

    let mut visited = vec![false; roots.len()];
    let mut order = Vec::with_capacity(roots.len());
    let mut next_file = 0usize;
    while order.len() < roots.len() {
        let mut seed = None;
        for _ in 0..file_count {
            let file = next_file;
            next_file = (next_file + 1) % file_count.max(1);
            seed = roots.iter().enumerate().find_map(|(index, root)| {
                (root.source_file == file && !visited[index]).then_some(index)
            });
            if seed.is_some() {
                break;
            }
        }
        let Some(seed) = seed.or_else(|| visited.iter().position(|&root_visited| !root_visited))
        else {
            break;
        };
        visit(
            seed,
            roots,
            selections,
            &records,
            &source_roots,
            &last_boundary_positions,
            &reference_record_positions,
            &attached,
            &mut visited,
            &mut order,
        );
    }

    let mut original: Vec<Option<CommonRoot>> = roots.drain(..).map(Some).collect();
    roots.extend(order.into_iter().filter_map(|index| original[index].take()));
}

fn trim_common_selections(
    selections: &mut Vec<CommonSelection>,
    coverage: &[Vec<bool>],
    trim_reference_factor: i32,
) {
    let trim = i64::from(trim_reference_factor);
    loop {
        let reference_scores: Vec<i64> = selections
            .iter()
            .map(|selection| {
                let locally_matched = coverage[selection.target_file]
                    [selection.target_position..selection.target_position + selection.length]
                    .iter()
                    .filter(|&&matched| matched)
                    .count() as i64;
                8 * selection.length as i64 - trim - 7 * locally_matched
            })
            .collect();

        let mut remove = vec![false; selections.len()];
        for (index, &score) in reference_scores.iter().enumerate() {
            if score < 0 {
                remove[index] = true;
            }
        }

        let mut sources = HashMap::<(usize, usize), (usize, i64, Vec<usize>)>::new();
        for (index, selection) in selections.iter().enumerate() {
            if remove[index] {
                continue;
            }
            let source = sources
                .entry((selection.source_file, selection.source_position))
                .or_insert_with(|| (0, -trim, Vec::new()));
            source.0 = source.0.max(selection.length);
            source.1 += reference_scores[index];
            source.2.push(index);
        }

        for ((source_file, source_position), (length, mut score, attached)) in sources {
            let locally_matched = coverage[source_file][source_position..source_position + length]
                .iter()
                .filter(|&&matched| matched)
                .count() as i64;
            score -= 7 * locally_matched;
            if score < 0 {
                for index in attached {
                    remove[index] = true;
                }
            }
        }

        if !remove.iter().any(|&removed| removed) {
            break;
        }
        let old_len = selections.len();
        let mut index = 0usize;
        selections.retain(|_| {
            let keep = !remove[index];
            index += 1;
            keep
        });
        if selections.len() == old_len || selections.is_empty() {
            break;
        }
    }
}

fn local_match_coverage(input: &[u8], settings: RangeSettings) -> Result<Vec<bool>> {
    let mut coverage = vec![false; input.len()];
    let mut recent_offsets = [0usize; 4];
    let window = 1usize
        .checked_shl(u32::from(settings.win_size))
        .unwrap_or(usize::MAX);
    let mut parser = LazyLzParser::new(window);
    let mut position = 0usize;
    while let Some(decision) = parser.next(
        input,
        MatchCost {
            scoring: MatchScoring::Heuristic,
            recent_offsets: &recent_offsets,
        },
    )? {
        match decision {
            LzDecision::Literal { .. } => position += 1,
            LzDecision::Match { length, distance } => {
                coverage[position..position + length].fill(true);
                encode_recent_distance(distance, &mut recent_offsets);
                position += length;
            }
        }
    }
    Ok(coverage)
}

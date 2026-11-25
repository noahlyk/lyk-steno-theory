//! How short: the brevity steps (`[[brevity]]` in the rules) that write a word in fewer strokes
//! than its plain reading, down to one-stroke skeletons. See `site/reference/brevity.json`.

use super::{Group, Rules, Shortening, Split, Step, Syllable, plain};
use crate::stroke::Stroke;

impl Rules {
    /// The shortening steps, in the order they are tried.
    pub fn brevity(&self) -> &[Shortening] {
        &self.brevity
    }

    /// The shorter outlines a shortening step gives a word of several syllables.
    pub fn shorten(&self, step: Shortening, word: &str, sounds: &[String]) -> Vec<Vec<Stroke>> {
        self.shorten_split(step, word, sounds).into_iter().map(|(outline, _)| outline).collect()
    }

    /// [`Self::shorten`], each outline with the groups its strokes were written with.
    pub fn shorten_split(&self, step: Shortening, word: &str, sounds: &[String]) -> Vec<(Vec<Stroke>, Split)> {
        let syllables = sounds
            .iter()
            .filter(|s| s.ends_with(|c: char| c.is_ascii_digit()))
            .count();
        if syllables < step.min_syllables {
            return Vec::new();
        }
        match step.step {
            Step::Lead | Step::Lean => self.leading(step.step, word, sounds).into_iter().collect(),
            Step::Fold => self
                .outline_folded_split(word, sounds, false, true)
                .into_iter()
                .collect(),
            Step::Skeleton | Step::Shell | Step::Ends => self
                .skeletons_split(word, sounds, step.any)
                .into_iter()
                .filter(|(kind, vowel, _, _)| *kind == step.step && *vowel == step.vowel)
                .map(|(_, _, stroke, groups)| (vec![stroke], vec![groups]))
                .collect(),
        }
    }

    /// A word of several syllables as one stroke: the consonants that start it, then consonants
    /// after its first vowel folded onto the right hand, with the stressed vowel or without. Which
    /// consonants depends on the step: all of them, those of the stressed and last syllables, or
    /// those of the last syllable only.
    #[cfg(test)]
    pub fn skeletons(&self, word: &str, sounds: &[String]) -> Vec<(Step, bool, Stroke)> {
        self.skeletons_split(word, sounds, false).into_iter().map(|(step, vowel, stroke, _)| (step, vowel, stroke)).collect()
    }

    /// The one-stroke skeletons, with the consonants allowed out of steno order (`any`) or not,
    /// each stroke with the groups it was written with.
    fn skeletons_split(&self, word: &str, sounds: &[String], any: bool) -> Vec<(Step, bool, Stroke, Vec<Group>)> {
        let labelled = self.label(word, sounds);
        let (prefix, skip) = self.prefix(word, &labelled).unzip();
        let Some(syllables) = self.syllables(&labelled[skip.unwrap_or(0)..]) else {
            return Vec::new();
        };
        if syllables.len() + usize::from(prefix.is_some()) < 2 {
            return Vec::new();
        }
        let Some((onset, start, onset_right, onset_groups)) = self.skeleton_left(&syllables) else { return Vec::new() };
        let onset = Stroke(onset.0 | prefix.map_or(0, |p| p.0));
        let mut left_groups: Vec<Group> = Vec::new();
        if let Some(p) = prefix {
            left_groups.push((p.0 & !Stroke::STAR, labelled[..skip.unwrap_or(0)].to_vec()));
        }
        left_groups.extend(onset_groups);
        let syllables = &syllables[start..];
        let stress = syllables.iter().position(|s| s.vowel.ends_with('1')).unwrap_or(0);
        let choices = skeleton_choices(syllables, stress, onset_right);

        // The vowel of the syllable with the main stress
        let key = syllables[stress].vowel.as_str();
        let vowel = self
            .vowel
            .get(key)
            .or_else(|| {
                self.vowel
                    .get(key.trim_end_matches(|c: char| c.is_ascii_digit()))
            })
            .or_else(|| self.vowel.get(plain(key)))
            .copied();

        let mut found: Vec<(Step, bool, Stroke, Vec<Group>)> = Vec::new();
        for (step, rest) in choices {
            // The consonants in steno order, as they are read; a form with no left hand and no
            // vowel is not offered
            let Some((coda, coda_groups)) = Self::consonant_groups(&rest, &self.coda, &self.coda_cluster, any) else {
                continue;
            };
            if coda.is_empty() {
                continue;
            }
            if onset.is_empty() {
                continue;
            }
            let bare = Stroke(onset.0 | coda.0);
            let mut groups = left_groups.clone();
            groups.extend(coda_groups.iter().cloned());
            found.push((step, false, bare, groups));
            if let Some(vowel) = vowel
                && vowel.0 & coda.0 & Stroke::STAR == 0
            {
                let mut groups = left_groups.clone();
                groups.push((vowel.0 & !Stroke::STAR, vec![key.to_owned()]));
                groups.extend(coda_groups);
                found.push((step, true, Stroke(bare.0 | vowel.0), groups));
            }
        }
        found
    }

    /// The outlines the `lead` and `lean` steps give a word: an unstressed first syllable is not
    /// written as a syllable. `lead` drops one that is only a vowel (`about` PWOUT, `again`
    /// TKPWEPB); `lean` puts the consonants of one onto the left hand of the next stroke when
    /// they come before its own keys (`believe` PWHRAOEF, `deliver` TKHREUFR).
    pub(super) fn leading(&self, step: Step, word: &str, sounds: &[String]) -> Option<(Vec<Stroke>, Split)> {
        let labelled = self.label(word, sounds);
        if self.prefix(word, &labelled).is_some() {
            return None;
        }
        let syllables = self.syllables(&labelled)?;
        let first = syllables.first()?;
        if syllables.len() < 2 || !first.vowel.ends_with('0') {
            return None;
        }
        let mut consonants = first.onset.clone();
        consonants.extend(first.coda.iter().cloned());
        let (mut rest, mut split): (Vec<Stroke>, Split) = self.strokes_groups(&syllables[1..], self.fold_any)?.into_iter().unzip();
        match step {
            Step::Lead if consonants.is_empty() => Some((rest, split)),
            Step::Lean if !consonants.is_empty() => {
                let (keys, groups) = Self::consonant_groups(&consonants, &self.onset, &self.onset_cluster, false)?;
                let next = rest.first_mut()?;
                let own = Stroke(next.0 & Stroke::LEFT);
                let in_order = match (keys.highest(), own.lowest()) {
                    (Some(high), Some(low)) => high <= low,
                    _ => true,
                };
                if !in_order {
                    return None;
                }
                next.0 |= keys.0;
                let first = split.first_mut()?;
                first.splice(0..0, groups);
                Some((rest, split))
            }
            _ => None,
        }
    }

    /// A skeleton's left hand, the first syllable it keeps, and whether that syllable's own onset
    /// goes on the right hand with the rest. An unstressed first syllable is not a syllable here:
    /// its consonants are the left hand (`in-` TPH, `be-` PW) and the next syllable's onset joins
    /// them when it comes after in order (`believe` PWHR); with no consonants it is dropped
    /// (`opinion` P-PB). `None` when the first kept onset can't be written.
    fn skeleton_left(&self, syllables: &[Syllable]) -> Option<(Stroke, usize, bool, Vec<Group>)> {
        let in_order = |a: Stroke, b: Stroke| match (a.highest(), b.lowest()) {
            (Some(high), Some(low)) => high <= low,
            _ => true,
        };
        let left = |sounds: &[String]| Self::consonant_groups(sounds, &self.onset, &self.onset_cluster, false);
        if syllables.len() >= 2 && syllables[0].vowel.ends_with('0') {
            let lead = &syllables[0];
            let coda: Vec<String> = lead.coda.iter().filter(|s| plain(s) != "Y").cloned().collect();
            let (mut keys, mut groups) = left(&lead.onset).unwrap_or_default();
            if let Some((c, more)) = left(&coda)
                && in_order(keys, c)
            {
                keys.0 |= c.0;
                groups.extend(more);
            }
            match left(&syllables[1].onset) {
                Some((next, more)) if in_order(keys, next) => {
                    groups.extend(more);
                    return Some((Stroke(keys.0 | next.0), 1, false, groups));
                }
                _ if keys.is_empty() => {}
                _ => return Some((keys, 1, true, groups)),
            }
        }
        let (keys, groups) = left(&syllables[0].onset)?;
        Some((keys, 0, false, groups))
    }
}

/// The consonants of a syllable, in order, with the `R` a reduced `ER` hides and without the glide
/// `Y` of `opinion`, `brilliant`.
fn consonants_of(syllable: &Syllable, with_onset: bool) -> Vec<String> {
    let mut sounds: Vec<String> = Vec::new();
    if with_onset {
        sounds.extend(syllable.onset.iter().cloned());
    }
    if plain(&syllable.vowel) == "ER" {
        sounds.push("R".into());
    }
    sounds.extend(syllable.coda.iter().cloned());
    sounds.retain(|s| plain(s) != "Y");
    sounds
}

/// Which consonants each one-stroke step keeps, from the most to the fewest: all of them
/// (`skeleton`), those of the stressed and last syllables (`shell`), those of the last only
/// (`ends`).
fn skeleton_choices(syllables: &[Syllable], stress: usize, onset_right: bool) -> Vec<(Step, Vec<String>)> {
    let last = syllables.len() - 1;
    let first_coda = consonants_of(&syllables[0], onset_right);
    let mut all = first_coda.clone();
    for syllable in &syllables[1..] {
        all.extend(consonants_of(syllable, true));
    }
    let mut some = if stress == 0 { first_coda } else { consonants_of(&syllables[stress], true) };
    if last > stress {
        some.extend(consonants_of(&syllables[last], true));
    }
    let mut choices = vec![(Step::Skeleton, all), (Step::Shell, some)];
    if last > 0 {
        choices.push((Step::Ends, consonants_of(&syllables[last], true)));
    }
    choices
}

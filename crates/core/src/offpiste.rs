//! Off-piste routing on a cost grid (C3, #35; Pixel benchmark C4, #36): A* on the 5 m elevation grid, avoids slopes over 30°, refuses zones forbidden at the date, weighted by estimated snow.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {
        assert_eq!(module_path!(), "skimap_core::offpiste::tests");
    }
}

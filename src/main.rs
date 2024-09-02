use std::error::Error;

use spectrum::run;

fn main() -> Result<(), Box<dyn Error>> {
    pollster::block_on(run())
}

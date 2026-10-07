mod common;
mod input;
mod oracle;
mod prefix;
mod prompt;
mod test;

use common::*;
use prompt::{PromptBuilder, PromptTemplate};

pub use input::inputprompts;
pub use oracle::oracleprompts;
pub use prefix::prefixprompts;
pub use prompt::Prompt;
pub use test::testprompts;

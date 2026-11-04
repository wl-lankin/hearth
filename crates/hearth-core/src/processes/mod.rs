//! A site's long-running dev processes (Vite, a queue worker, Pail): saved per site, run by the
//! supervisor in the site's folder.

pub mod store;
pub mod suggest;

#[cfg(test)]
mod tests;

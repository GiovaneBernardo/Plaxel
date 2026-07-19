use engine::{ecs::entity::Entity, math::Vec3};

// Universe origin assumes 0 0 0, no need for a displaced universe?
pub struct Universe {}

pub struct StarSystemComponent {
    pub planets: Vec<Entity>,
    pub radius: f64,
    pub surface_temperature: f64,
    pub mass: f64,

    // Cache calculated by star params
    pub emission_color: Vec3,
    pub luminosity: f64,
}

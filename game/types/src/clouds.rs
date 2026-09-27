#[derive(Clone, Debug, plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub struct CloudsComponent {
    pub min_height: f32,
    pub max_height: f32,
}

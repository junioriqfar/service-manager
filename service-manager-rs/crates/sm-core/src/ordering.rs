use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::ServiceConfig;

struct Graph {
    indegree: Vec<usize>,
    dependents: Vec<Vec<usize>>,
}

fn build_graph(services: &[ServiceConfig]) -> Graph {
    let count = services.len();
    let mut index_by_name: HashMap<String, usize> = HashMap::new();
    for (index, service) in services.iter().enumerate() {
        index_by_name
            .entry(service.name.trim().to_lowercase())
            .or_insert(index);
    }

    let mut indegree = vec![0usize; count];
    let mut dependents = vec![Vec::new(); count];

    for (index, service) in services.iter().enumerate() {
        for dependency in service.normalized_dependencies() {
            if let Some(&dependency_index) = index_by_name.get(&dependency.to_lowercase()) {
                if dependency_index == index {
                    continue;
                }
                indegree[index] += 1;
                dependents[dependency_index].push(index);
            }
        }
    }

    Graph {
        indegree,
        dependents,
    }
}

/// Urutan start berdasarkan dependensi (topological sort).
///
/// Service tanpa dependensi diproses lebih dulu. Bila terjadi siklus, service yang
/// tersisa ditambahkan di akhir dengan urutan asli agar perilaku tetap deterministik.
pub fn start_order(services: &[ServiceConfig]) -> Vec<usize> {
    let graph = build_graph(services);
    let mut indegree = graph.indegree;
    let mut queue: VecDeque<usize> = (0..services.len())
        .filter(|&index| indegree[index] == 0)
        .collect();
    let mut ordered = Vec::with_capacity(services.len());
    let mut added = vec![false; services.len()];

    while let Some(index) = queue.pop_front() {
        if added[index] {
            continue;
        }
        added[index] = true;
        ordered.push(index);
        for &dependent in &graph.dependents[index] {
            if indegree[dependent] > 0 {
                indegree[dependent] -= 1;
            }
            if indegree[dependent] == 0 && !added[dependent] {
                queue.push_back(dependent);
            }
        }
    }

    for (index, was_added) in added.iter().enumerate() {
        if !was_added {
            ordered.push(index);
        }
    }

    ordered
}

/// Urutan stop adalah kebalikan dari urutan start.
pub fn stop_order(services: &[ServiceConfig]) -> Vec<usize> {
    let mut order = start_order(services);
    order.reverse();
    order
}

/// Urutan start per lapisan. Service dalam satu lapisan tidak saling bergantung
/// sehingga aman dijalankan paralel; lapisan berikutnya menunggu lapisan sebelumnya.
pub fn start_layers(services: &[ServiceConfig]) -> Vec<Vec<usize>> {
    let graph = build_graph(services);
    let mut indegree = graph.indegree;
    let mut remaining: HashSet<usize> = (0..services.len()).collect();
    let mut layers: Vec<Vec<usize>> = Vec::new();

    let mut current: Vec<usize> = (0..services.len())
        .filter(|&index| indegree[index] == 0)
        .collect();

    while !current.is_empty() {
        current.sort_unstable();
        current.dedup();
        for &index in &current {
            remaining.remove(&index);
        }
        layers.push(current.clone());

        let mut next: Vec<usize> = Vec::new();
        for &index in &current {
            for &dependent in &graph.dependents[index] {
                if indegree[dependent] > 0 {
                    indegree[dependent] -= 1;
                }
                if indegree[dependent] == 0 && remaining.contains(&dependent) {
                    next.push(dependent);
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        current = next;
    }

    // Siklus: masukkan sisa node sebagai lapisan terakhir dengan urutan asli.
    if !remaining.is_empty() {
        let leftovers: Vec<usize> = (0..services.len())
            .filter(|index| remaining.contains(index))
            .collect();
        layers.push(leftovers);
    }

    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, dependencies: &[&str]) -> ServiceConfig {
        ServiceConfig {
            name: name.to_string(),
            dependencies: dependencies.iter().map(|value| value.to_string()).collect(),
            ..ServiceConfig::default()
        }
    }

    fn names(services: &[ServiceConfig], order: &[usize]) -> Vec<String> {
        order
            .iter()
            .map(|&index| services[index].name.clone())
            .collect()
    }

    #[test]
    fn independent_services_keep_input_order() {
        let services = vec![service("a", &[]), service("b", &[]), service("c", &[])];
        assert_eq!(names(&services, &start_order(&services)), ["a", "b", "c"]);
    }

    #[test]
    fn dependency_starts_first() {
        let services = vec![
            service("app", &["postgresql", "redis"]),
            service("postgresql", &[]),
            service("redis", &[]),
        ];
        let order = start_order(&services);
        assert_eq!(names(&services, &order), ["postgresql", "redis", "app"]);
    }

    #[test]
    fn stop_order_is_reversed() {
        let services = vec![service("app", &["postgresql"]), service("postgresql", &[])];
        let order = stop_order(&services);
        assert_eq!(names(&services, &order), ["app", "postgresql"]);
    }

    #[test]
    fn missing_and_self_dependencies_are_ignored() {
        let services = vec![
            service("app", &["does-not-exist", "app"]),
            service("worker", &["app"]),
        ];
        assert_eq!(names(&services, &start_order(&services)), ["app", "worker"]);
    }

    #[test]
    fn cycles_fall_back_to_input_order() {
        let services = vec![service("a", &["b"]), service("b", &["a"])];
        assert_eq!(names(&services, &start_order(&services)), ["a", "b"]);
    }

    #[test]
    fn layers_group_independent_services() {
        let services = vec![
            service("app", &["db"]),
            service("db", &[]),
            service("cache", &[]),
            service("worker", &["app", "cache"]),
        ];
        let layers = start_layers(&services);
        assert_eq!(layers.len(), 3);
        assert_eq!(names(&services, &layers[0]), ["db", "cache"]);
        assert_eq!(names(&services, &layers[1]), ["app"]);
        assert_eq!(names(&services, &layers[2]), ["worker"]);
    }

    #[test]
    fn layers_include_cycles_in_final_layer() {
        let services = vec![
            service("a", &["b"]),
            service("b", &["a"]),
            service("c", &[]),
        ];
        let layers = start_layers(&services);
        assert_eq!(layers.len(), 2);
        assert_eq!(names(&services, &layers[0]), ["c"]);
        assert_eq!(names(&services, &layers[1]), ["a", "b"]);
    }
}

#ifndef SYCL_TERNARY_SPIKFORMER_HPP
#define SYCL_TERNARY_SPIKFORMER_HPP

#include <sycl/sycl.hpp>
#include <cstdint>

// Ternary type: -1, 0, 1
typedef int8_t ternary_t;

#ifdef __cplusplus
extern "C" {
#endif

// Ternary matrix multiplication
int sycl_ternary_matmul(
    const ternary_t* a_data,
    const ternary_t* b_data,
    ternary_t* c_data,
    int m, int n, int k
);

// Ternary attention mechanism
int sycl_ternary_attention(
    const ternary_t* q_data,
    const ternary_t* k_data,
    const ternary_t* v_data,
    ternary_t* output_data,
    ternary_t* attention_weights,
    int batch_size,
    int num_heads,
    int seq_len,
    int head_dim
);

// Population coding for ternary conversion
int sycl_population_coding(
    const float* input_data,
    ternary_t* ternary_data,
    int n,
    float threshold_pos,
    float threshold_neg
);

// Ternary spiking neuron simulation
int sycl_ternary_neuron(
    const ternary_t* input_spikes,
    ternary_t* output_spikes,
    const ternary_t* weights,
    int32_t* membrane_potential,
    int batch_size,
    int seq_len,
    int num_neurons
);

#ifdef __cplusplus
}
#endif

#endif // SYCL_TERNARY_SPIKFORMER_HPP
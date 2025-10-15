#include "sycl_ternary_spikformer.hpp"
#include <sycl/sycl.hpp>
#include <cstdint>
#include <iostream>
#include <cmath>

// Simple integer LIF params
static constexpr int32_t DECAY_NUM = 7;
static constexpr int32_t DECAY_DEN = 8;
static constexpr int32_t THRESH = 16;
static constexpr int32_t RESET = 16;

extern "C" {

int sycl_ternary_matmul(
    const ternary_t* a_data,
    const ternary_t* b_data,
    ternary_t* c_data,
    int m, int n, int k
) {
    try {
        sycl::queue q;
        
        sycl::buffer<ternary_t, 1> a_buf(a_data, sycl::range<1>(m * k), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> b_buf(b_data, sycl::range<1>(k * n), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> c_buf(c_data, sycl::range<1>(m * n), {sycl::property::buffer::use_host_ptr()});
        
        q.submit([&](sycl::handler& h) {
            auto a_acc = a_buf.get_access<sycl::access::mode::read>(h);
            auto b_acc = b_buf.get_access<sycl::access::mode::read>(h);
            auto c_acc = c_buf.get_access<sycl::access::mode::write>(h);
            
            auto wg_size = sycl::range<2>(16, 16);
            auto global_size = sycl::range<2>(
                (m + wg_size[0] - 1) / wg_size[0] * wg_size[0],
                (n + wg_size[1] - 1) / wg_size[1] * wg_size[1]
            );
            
            h.parallel_for(sycl::nd_range<2>(global_size, wg_size),
                          [=](sycl::nd_item<2> item) {
                int row = item.get_global_id(0);
                int col = item.get_global_id(1);
                
                if (row < m && col < n) {
                    int sum = 0;
                    for (int i = 0; i < k; i++) {
                        ternary_t a_val = a_acc[row * k + i];
                        ternary_t b_val = b_acc[i * n + col];
                        sum += a_val * b_val;
                    }
                    c_acc[row * n + col] = (sum > 0) ? 1 : (sum < 0) ? -1 : 0;
                }
            });
        });
        
        q.wait();
        return 0; // Success
    } catch (const std::exception& e) {
        std::cerr << "SYCL ternary matmul error: " << e.what() << std::endl;
        return -1; // Error
    }
}

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
) {
    try {
        sycl::queue q;
        
        sycl::buffer<ternary_t, 1> q_buf(q_data, sycl::range<1>(batch_size * num_heads * seq_len * head_dim), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> k_buf(k_data, sycl::range<1>(batch_size * num_heads * seq_len * head_dim), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> v_buf(v_data, sycl::range<1>(batch_size * num_heads * seq_len * head_dim), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> output_buf(output_data, sycl::range<1>(batch_size * num_heads * seq_len * head_dim), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> attn_buf(attention_weights, sycl::range<1>(batch_size * num_heads * seq_len * seq_len), {sycl::property::buffer::use_host_ptr()});
        
        // Compute attention scores
        q.submit([&](sycl::handler& h) {
            auto q_acc = q_buf.get_access<sycl::access::mode::read>(h);
            auto k_acc = k_buf.get_access<sycl::access::mode::read>(h);
            auto attn_acc = attn_buf.get_access<sycl::access::mode::write>(h);
            
            h.parallel_for(sycl::range<4>(batch_size, num_heads, seq_len, seq_len),
                          [=](sycl::id<4> idx) {
                int b = idx[0], head = idx[1], i = idx[2], j = idx[3];
                
                int score = 0;
                for (int d = 0; d < head_dim; d++) {
                    int q_idx = ((b * num_heads + head) * seq_len + i) * head_dim + d;
                    int k_idx = ((b * num_heads + head) * seq_len + j) * head_dim + d;
                    score += q_acc[q_idx] * k_acc[k_idx];
                }
                
                int attn_idx = ((b * num_heads + head) * seq_len + i) * seq_len + j;
                attn_acc[attn_idx] = (score > head_dim/4) ? 1 : (score < -head_dim/4) ? -1 : 0;
            });
        });
        
        // Apply attention to values
        q.submit([&](sycl::handler& h) {
            auto attn_acc = attn_buf.get_access<sycl::access::mode::read>(h);
            auto v_acc = v_buf.get_access<sycl::access::mode::read>(h);
            auto output_acc = output_buf.get_access<sycl::access::mode::write>(h);
            
            h.parallel_for(sycl::range<4>(batch_size, num_heads, seq_len, head_dim),
                          [=](sycl::id<4> idx) {
                int b = idx[0], head = idx[1], i = idx[2], d = idx[3];
                
                int sum = 0;
                for (int j = 0; j < seq_len; j++) {
                    int attn_idx = ((b * num_heads + head) * seq_len + i) * seq_len + j;
                    int v_idx = ((b * num_heads + head) * seq_len + j) * head_dim + d;
                    sum += attn_acc[attn_idx] * v_acc[v_idx];
                }
                
                int output_idx = ((b * num_heads + head) * seq_len + i) * head_dim + d;
                output_acc[output_idx] = (sum > 0) ? 1 : (sum < 0) ? -1 : 0;
            });
        });
        
        q.wait();
        return 0; // Success
    } catch (const std::exception& e) {
        std::cerr << "SYCL ternary attention error: " << e.what() << std::endl;
        return -1; // Error
    }
}

int sycl_population_coding(
    const float* input_data,
    ternary_t* ternary_data,
    int n,
    float threshold_pos,
    float threshold_neg
) {
    try {
        sycl::queue q;
        
        sycl::buffer<float, 1> input_buf(input_data, sycl::range<1>(n), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> ternary_buf(ternary_data, sycl::range<1>(n), {sycl::property::buffer::use_host_ptr()});
        
        q.submit([&](sycl::handler& h) {
            auto input_acc = input_buf.get_access<sycl::access::mode::read>(h);
            auto ternary_acc = ternary_buf.get_access<sycl::access::mode::write>(h);
            
            h.parallel_for(sycl::range<1>(n), [=](sycl::id<1> idx) {
                float val = input_acc[idx];
                if (val > threshold_pos) {
                    ternary_acc[idx] = 1;
                } else if (val < threshold_neg) {
                    ternary_acc[idx] = -1;
                } else {
                    ternary_acc[idx] = 0;
                }
            });
        });
        
        q.wait();
        return 0; // Success
    } catch (const std::exception& e) {
        std::cerr << "SYCL population coding error: " << e.what() << std::endl;
        return -1; // Error
    }
}

int sycl_ternary_neuron(
    const ternary_t* input_spikes,
    ternary_t* output_spikes,
    const ternary_t* weights,
    int32_t* membrane_potential,
    int batch_size,
    int seq_len,
    int num_neurons
) {
    try {
        sycl::queue q;
        
        sycl::buffer<ternary_t, 1> in_buf(input_spikes, sycl::range<1>(batch_size * seq_len * num_neurons), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> out_buf(output_spikes, sycl::range<1>(batch_size * seq_len * num_neurons), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<ternary_t, 1> w_buf(weights, sycl::range<1>(num_neurons * num_neurons), {sycl::property::buffer::use_host_ptr()});
        sycl::buffer<int32_t, 1> v_buf(membrane_potential, sycl::range<1>(batch_size * num_neurons), {sycl::property::buffer::use_host_ptr()});
        
        q.submit([&](sycl::handler& h) {
            auto in = in_buf.get_access<sycl::access::mode::read>(h);
            auto out = out_buf.get_access<sycl::access::mode::write>(h);
            auto W = w_buf.get_access<sycl::access::mode::read>(h);
            auto V = v_buf.get_access<sycl::access::mode::read_write>(h);
            
            h.parallel_for(sycl::range<2>(batch_size, num_neurons), [=](sycl::id<2> ij) {
                int b = ij[0], n = ij[1];
                int32_t v = V[b * num_neurons + n];
                
                for (int t = 0; t < seq_len; ++t) {
                    int32_t current = 0;
                    const int base_in = (b * seq_len + t) * num_neurons;
                    for (int j = 0; j < num_neurons; ++j) {
                        current += static_cast<int32_t>(W[j * num_neurons + n]) * static_cast<int32_t>(in[base_in + j]);
                    }
                    
                    v = (DECAY_NUM * v) / DECAY_DEN + current;
                    
                    ternary_t s = 0;
                    if (v >= THRESH) {
                        s = 1;
                        v -= RESET;
                    } else if (v <= -THRESH) {
                        s = -1;
                        v += RESET;
                    }
                    out[base_in + n] = s;
                }
                V[b * num_neurons + n] = v;
            });
        });
        
        q.wait();
        return 0; // Success
    } catch (const std::exception& e) {
        std::cerr << "SYCL ternary neuron error: " << e.what() << std::endl;
        return -1; // Error
    }
}

} // extern "C"
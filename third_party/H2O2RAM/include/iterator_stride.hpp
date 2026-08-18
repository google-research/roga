#pragma once
#include <concepts>
#include <cstddef>
#include <iterator>
#include <type_traits>
#include <utility>

namespace ORAM {
template <typename T>
class IteratorStride {
  using this_type = IteratorStride<T>;

 public:
  using pointer = T*;
  using reference = T&;
  using value_type = typename std::remove_cv<T>::type;
  using difference_type = std::ptrdiff_t;
  using iterator_category = std::random_access_iterator_tag;

  IteratorStride() : ptr_(nullptr), stride_(0) {}

  template <typename RandomAccessIter>
    requires(!std::is_same_v<std::remove_cvref_t<RandomAccessIter>,
                             IteratorStride> &&
             std::random_access_iterator<std::remove_cvref_t<RandomAccessIter>>)
  IteratorStride(RandomAccessIter iter, int64_t stride = 0)
      : ptr_(&*iter), stride_(stride) {}

  IteratorStride(this_type&& other) = default;
  IteratorStride(const this_type& other) = default;
  IteratorStride(this_type& other) = default;
  this_type& operator=(const this_type& other) = default;
  this_type& operator=(this_type&& other) = default;

  reference operator*() const { return *ptr_; }

  pointer operator->() const { return ptr_; }

  // Prefix increment
  this_type& operator++() {
    ptr_ += stride_;
    return *this;
  }

  // Postfix increment
  this_type operator++(int) {
    this_type tmp = *this;
    ++(*this);
    return tmp;
  }

  // Prefix increment
  this_type& operator--() {
    ptr_ -= stride_;
    return *this;
  }

  // Postfix increment
  this_type operator--(int) {
    this_type tmp = *this;
    --(*this);
    return tmp;
  }

  // Comparison operators
  bool operator==(const this_type& other) const { return ptr_ == other.ptr_; }

  bool operator<(const this_type& other) const { return ptr_ < other.ptr_; }

  bool operator>(const this_type& other) const { return ptr_ > other.ptr_; }

  bool operator<=(const this_type& other) const { return ptr_ <= other.ptr_; }

  bool operator>=(const this_type& other) const { return ptr_ >= other.ptr_; }

  bool operator!=(const this_type& other) const { return !(*this == other); }

  difference_type operator-(const this_type& other) const {
    return (ptr_ - other.ptr_) / stride_;
  }

  this_type operator-(difference_type n) const {
    return this_type(ptr_ - (stride_ * n), stride_);
  }

  this_type& operator-=(difference_type n) {
    ptr_ -= stride_ * n;
    return *this;
  }

  this_type operator+(difference_type n) const {
    return this_type(ptr_ + (stride_ * n), stride_);
  }

  friend this_type operator+(difference_type n, const this_type& it) {
    return it + n;
  }

  this_type& operator+=(difference_type n) {
    ptr_ += stride_ * n;
    return *this;
  }

  reference operator[](difference_type n) const { return ptr_[n * stride_]; }

  int64_t get_stride() const { return stride_; }

  void set_stride(int64_t stride) { stride_ = stride; }

  void reverse() { stride_ = -stride_; }

 private:
  pointer ptr_;
  int64_t stride_;
};
}  // namespace ORAM
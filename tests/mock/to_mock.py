counter = 0


def function_to_mock():
    global counter
    counter += 1
    return counter


def main():
    function_to_mock()
    res = function_to_mock()
    return res

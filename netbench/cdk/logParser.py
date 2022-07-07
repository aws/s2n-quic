import sys

def main(file_name):
    f = open(file_name, "r+")
    lines = [line[line.index('{'):] for line in f.readlines()]
    f.truncate(0)
    f.seek(0)
    f.writelines(lines)
    f.close()

if __name__ == "__main__":
    main(sys.argv[1])